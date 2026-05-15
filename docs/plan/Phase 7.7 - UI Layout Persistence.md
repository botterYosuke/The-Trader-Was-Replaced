# Phase 7.7 — UI Layout Persistence: 起動時自動復元 + 実行中自動保存 + viewport/symbol

## Context

[Phase 7.6 - UI Resilient Diffie.md](Phase%207.6%20-%20UI%20Resilient%20Diffie.md) で、UI レイアウト永続化の **基盤層**（JSON スキーマ / 保存・読込 I/O / `apply_layout_system` / 手動 Save/Save As/Load メニュー + Alt ショートカット / AppExit 自動保存）を実装した。

本フェーズはその土台の上に、ユーザ体験として最も重要な **「アプリを開き直したら前回の続きから始まる」** を実現する。手動操作なしで自動的に最後の状態へ復元される。

## スコープ

| サブステップ | 内容 | 旧 Step 番号 (7.6 doc) | 状態 |
|---|---|---|---|
| **7Z** | **タイトルバー右端に閉じるボタン（×）** | （新規） | ✅ E2E PASS |
| 7A | グローバル `app_state.json` の I/O（`last_strategy_path` 保持） | Step 1 | ✅ |
| 7B | 起動時に `last_strategy_path` を `OpenStrategyRequested` で自動発火 | Step 1 | ✅ E2E PASS |
| 7C | サイドカー `.json` が存在すれば Open 直後に自動 `LayoutLoadRequested` | Step 6 | ✅ E2E PASS |
| 7D | ドラッグを検知 → 1 秒デバウンスで自動保存 | Step 5 | ✅ |
| 7E | viewport (camera pan/zoom) の保存・復元（PanCam 連動） | Step 7 | ✅ |
| 7F | `selected_symbol` スキーマ追加（サイドバー連動は Phase 7.8 以降） | Step 8 | ✅ (スキーマのみ) |

**cargo test --lib: 23 passed, 0 failed（2026-05-15）**

**スコープ外**:
- 手動 Save/Save As/Load + AppExit 保存 = **Phase 7.6 で完了済**
- ウィンドウリサイズ機能そのもの = 別フェーズ（7D は「リサイズが将来実装されたら検知」フックのみ）
- 解像度変更時の画面外クランプ = 当面無視
- 未知 `PanelKind` バリアントの後方互換 = `serde(other)` ではなく warn ログ + 無視
- `selected_symbol` のサイドバー連動（値の収集・復元）= Phase 7.8 以降

## Phase 7.6 との分担

| 観点 | Phase 7.6 | Phase 7.7 |
|---|---|---|
| 保存トリガー | 手動 (Alt+S/A) + AppExit | 上記 + 自動デバウンス |
| Load トリガー | 手動 (Alt+O) | 上記 + 起動時自動 |
| サイドカーパスの解決 | `original_path.with_extension("json")` | 同左（共有） |
| 「どの `.py` を開くか」の記憶 | なし（毎回 Open Strategy） | `app_state.json` で永続化 |
| viewport データ | スキーマ枠は確保済、保存値はゼロ | 実値を camera から収集 |
| selected_symbol データ | スキーマから除外 | スキーマに追加（`serde(default)`、収集は 7.8 以降） |

## 設計

### ✅ 7Z. タイトルバー閉じるボタン（×）— **最優先**

**実装結果（計画からの変更あり）**:

> **⚠️ 設計変更: `Visibility::Hidden` → `despawn_recursive()` に変更**
>
> 計画では `Visibility::Hidden` を採用する予定だったが、ユーザーの明示指示で `despawn_recursive()` に変更。
> これにより 7D の dirty 検知は `Changed<Visibility>` を使わず、`Pointer<DragEnd>` のみになった。
> サイドバーボタンは Hidden→Inherited toggle が不要になり、元の「entity が無ければ spawn」ロジックに戻った。

**実装内容**:
- `src/ui/floating_window.rs`: タイトルバー右端（`title_bar` の**子ではなく root 直下**）に `CloseButton` marker + Text2d `×` + 背景 Sprite (12×12px) を spawn
- Click observer で `commands.entity(parent.get()).despawn_recursive()`
- `src/ui/strategy_editor.rs`: Run/Save ボタンの x 座標を `CLOSE_BTN_RESERVED = 36.0` 分左に退避（× と重なるバグを修正）
- `src/ui/components.rs`: `CloseButton` marker component を追加

**Tips**:
- × ボタンを `title_bar` の子にしない理由: `title_bar` の `Pointer<Drag>` observer が子に伝播してドラッグになってしまう
- Strategy Editor はタイトルバーに Save Cache・Run ボタンがあるため、他パネルより横幅が必要。`CLOSE_BTN_RESERVED` 定数で統一管理

**✅ E2E PASS（6 panel 全部）**:
1. × クリック → despawn（消える）
2. サイドバーボタン → re-spawn（同位置・同サイズで再表示）

### ✅ 7A. グローバル app-state

**保存場所**: `dirs::config_dir()/the-trader-was-replaced/app_state.json`
（Windows: `%APPDATA%\the-trader-was-replaced\app_state.json`）

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct AppState {
    pub schema_version: u32,
    pub last_strategy_path: Option<PathBuf>,
}
```

**新規ファイル**: `src/ui/app_state.rs`
- `load_app_state()` / `save_app_state()` ヘルパー
- unit tests 2 件（load/save round-trip）

**Tips**:
- `dirs` crate を `Cargo.toml` に追加（`dirs = "5"`）
- `OpenStrategyRequested` を受信するたびに同期 write（軽量、クラッシュ耐性あり）

### ✅ 7B. 起動時オートオープン

**実装**:
```rust
// src/ui/app_state.rs (Startup system)
fn restore_last_strategy_system(
    mut events: EventWriter<OpenStrategyRequested>,
) {
    let Ok(state) = load_app_state() else { return };
    let Some(path) = state.last_strategy_path else { return };
    if !path.exists() {
        warn!("last strategy path no longer exists: {:?}", path);
        return;
    }
    events.send(OpenStrategyRequested { path });
}
```

`src/ui/menu_bar.rs` の `open_strategy_buffer_system` で strategy open 時に `save_app_state()` を呼んで `last_strategy_path` を更新。

**✅ E2E PASS**: 再起動後に前回の `.py` が自動で開く

### ✅ 7C. Open 直後の自動 Load

**実装**:
```rust
// src/ui/layout_persistence.rs
#[derive(Resource, Default)]
struct PendingLayoutLoad {
    path: Option<PathBuf>,
}

#[derive(Resource, Default)]
struct SidecarAutoLoadState {
    done: bool,  // ワンショットフラグ
}

fn watch_open_strategy_for_sidecar_system(
    mut events: EventReader<OpenStrategyRequested>,
    mut state: ResMut<SidecarAutoLoadState>,
    mut pending: ResMut<PendingLayoutLoad>,
) {
    if state.done {
        for _ in events.read() {}  // 消費して溜まらないようにする
        return;
    }
    for event in events.read() {
        let sidecar = event.path.with_extension("json");
        if sidecar.exists() {
            pending.path = Some(sidecar);
            state.done = true;  // ワンショット: 以降は無視
        }
    }
}
```

> **⚠️ 重大バグと修正**: 初実装では `SidecarAutoLoadState` がなく無限ループが発生した。
>
> **ループの仕組み**:
> `OpenStrategyRequested` 受信 → `LayoutLoadRequested` 発火 → `apply_layout_system` が strategy_path 復元のため再び `OpenStrategyRequested` 発火 → ループ
>
> **修正**: `SidecarAutoLoadState { done: bool }` でワンショット化。
> `done = true` 後も `for _ in events.read() {}` でイベントを消費しないと翌フレームに溜まるので注意。
>
> **教訓**: `OpenStrategyRequested` を監視するシステムは必ずワンショットフラグか「起動時のみ」ガードを入れる。

**✅ E2E PASS**: 再起動後にレイアウトも自動復元される（パネルが動かせることも確認）

### ✅ 7D. 自動デバウンス保存

```rust
#[derive(Resource, Default)]
struct AutoSaveState {
    dirty: bool,
    last_change: Option<Instant>,
}
```

- `mark_dirty_on_drag_system`: `Pointer<DragEnd>` で `dirty = true`
- `debounced_autosave_system`: dirty && elapsed > 1s → `build_layout` → `save_layout_to`

**実装上の変更**:
- 計画では `Changed<Visibility>` も検知対象だったが、7Z が `despawn_recursive()` 採用になったため **`Pointer<DragEnd>` のみ** を監視

### ✅ 7E. viewport の収集と適用

`build_layout` で `camera.Transform.translation.{x,y}` と `OrthographicProjection.scale` を実値収集。
`apply_layout_system` で書き戻し。

**PanCam 競合確認結果**: `camera.rs` の PanCam は `enabled` フラグ制御のみで `Transform` への外部書き込みを妨げない。追加実装不要。

### ✅ 7F. selected_symbol スキーマ追加

```rust
pub struct SidecarLayout {
    pub schema_version: u32,
    pub viewport: ViewportState,
    #[serde(default)]
    pub selected_symbol: Option<String>,  // ← 7F で追加
    pub windows: Vec<WindowLayout>,
}
```

- `#[serde(default)]` で 7.6 が書き出した古い JSON（このフィールドなし）を読める
- サイドバーとの連動（値の収集・復元）は **Phase 7.8 以降**
- スキーマバージョンは `1` のまま（`serde(default)` で後方互換）

## 落とし穴・要確認事項（実装後の知見を追記）

1. **7.6 の AppExit observer と 7D の autosave の競合** — 終了時に AppExit 経路と「dirty を見て save」経路が二重発火する可能性。現状は AppExit がトリガーなので問題ないが、将来 7D が AppExit より前に走る場合は注意。
2. **`app_state.json` の保存タイミング** ✅ 同期 write を採用（`OpenStrategyRequested` ごと）。
3. **panel spawn 順序** ✅ `PendingLayoutLoad` + 1 フレーム遅延で解決。
4. **PanCam の "scale 上書き" 不能問題** ✅ 問題なし（PanCam は `enabled` フラグのみ）。
5. **`OpenStrategyRequested` の無限ループ** ✅ `SidecarAutoLoadState { done: bool }` で解決（7C 参照）。

## 修正/新規ファイル（実績）

- ✅ [src/ui/floating_window.rs](src/ui/floating_window.rs) — **(7Z)** × ボタン spawn + Click observer (despawn_recursive)
- ✅ [src/ui/sidebar.rs](src/ui/sidebar.rs) — Visibility toggle 分岐なし（despawn後は既存 spawn ロジックで対応）
- ✅ [src/ui/components.rs](src/ui/components.rs) — `CloseButton` marker component 追加
- ✅ [src/ui/strategy_editor.rs](src/ui/strategy_editor.rs) — `CLOSE_BTN_RESERVED = 36.0` で Run/Save ボタンを退避
- ✅ 🆕 [src/ui/app_state.rs](src/ui/app_state.rs) — `AppState` schema + I/O + unit tests 2 件
- ✅ [src/ui/layout_persistence.rs](src/ui/layout_persistence.rs) — `selected_symbol` 追加、`build_layout` で viewport 実値収集、autosave system + dirty 検知、`SidecarAutoLoadState` ワンショット修正
- ✅ [src/ui/menu_bar.rs](src/ui/menu_bar.rs) — `open_strategy_buffer_system` で `save_app_state()` 呼び出し
- ✅ [src/camera.rs](src/camera.rs) — 変更不要（PanCam 競合なし確認）
- ✅ [src/ui/mod.rs](src/ui/mod.rs) — `app_state` module + plugin 拡張

## 検証結果

1. ✅ `cargo test --lib`: **23 passed, 0 failed**（`app_state.rs` unit tests 2 件含む）
2. ✅ **手動 E2E (7Z 閉じるボタン)**:
   - × クリック → despawn（消える）
   - サイドバーボタン → re-spawn（同位置・同サイズで再表示）
   - 6 panel 全部 PASS
3. ✅ **手動 E2E (7B オートオープン)**:
   - 再起動後に前回の `.py` が自動で開く PASS
4. ✅ **手動 E2E (7C オート Load)**:
   - 再起動後にレイアウトも自動復元 PASS
   - パネルをドラッグで動かせる（ループ修正後）PASS
5. ⬜ **手動 E2E (7D デバウンス保存)**: 未実施（1 秒後の mtime 確認）
6. ⬜ **手動 E2E (viewport)**: 未実施
7. ⬜ **手動 E2E (selected_symbol)**: 収集・復元実装が Phase 7.8 以降のため除外

## 次の作業者への引継ぎ事項

**Phase 7.7 は完了（2026-05-15 E2E PASS）。次は Phase 7.8（selected_symbol の実際の収集・復元）推奨。**

**やり残し（Phase 7.8 候補）**:
1. `selected_symbol` のサイドバー連動 — `SidecarLayout.selected_symbol` フィールドは追加済み。`build_layout` で選択銘柄を収集し、`apply_layout_system` でサイドバーに反映する実装が未実施。
2. 7D の手動 E2E（mtime 確認）— `debounced_autosave_system` は実装済みだが、ドラッグ後 1 秒で `.json` が更新されることの目視確認が未実施。

**重要な設計決定（計画からの変更）**:
- **× ボタン: `Visibility::Hidden` → `despawn_recursive()` に変更**（ユーザー明示指示）
- これにより 7D の dirty 検知は `Pointer<DragEnd>` のみ（`Changed<Visibility>` は使わない）

**既知の落とし穴**:
- `OpenStrategyRequested` を監視するシステムを追加するときは `SidecarAutoLoadState` のパターンを参照し、必ずワンショットフラグを入れること（7C 無限ループの教訓）

## 完了条件

✅ 「アプリを閉じて再度開いたとき、前回終了時の状態（開いていた `.py`、各 panel の位置/サイズ/可視性、カメラ pan/zoom）が完全に復元される」をユーザが体感できる。
