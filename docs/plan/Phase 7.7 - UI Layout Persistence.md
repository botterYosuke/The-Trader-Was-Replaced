# Phase 7.7 — UI Layout Persistence: 起動時自動復元 + 実行中自動保存 + viewport/symbol

## Context

[Phase 7.6 - UI Resilient Diffie.md](Phase%207.6%20-%20UI%20Resilient%20Diffie.md) で、UI レイアウト永続化の **基盤層**（JSON スキーマ / 保存・読込 I/O / `apply_layout_system` / 手動 Save/Save As/Load メニュー + Alt ショートカット / AppExit 自動保存）を実装した。

本フェーズはその土台の上に、ユーザ体験として最も重要な **「アプリを開き直したら前回の続きから始まる」** を実現する。手動操作なしで自動的に最後の状態へ復元される。

## スコープ

| サブステップ | 内容 | 旧 Step 番号 (7.6 doc) |
|---|---|---|
| 7A | グローバル `app_state.json` の I/O（`last_strategy_path` 保持） | Step 1 |
| 7B | 起動時に `last_strategy_path` を `OpenStrategyRequested` で自動発火 | Step 1 |
| 7C | サイドカー `.json` が存在すれば Open 直後に自動 `LayoutLoadRequested` | Step 6 |
| 7D | ドラッグ/リサイズ/可視性変化を検知 → 1 秒デバウンスで自動保存 | Step 5 |
| 7E | viewport (camera pan/zoom) の保存・復元（PanCam 連動） | Step 7 |
| 7F | `selected_symbol` の保存・復元（サイドバー連動） | Step 8 |

**スコープ外**:
- 手動 Save/Save As/Load + AppExit 保存 = **Phase 7.6 で完了済**
- ウィンドウリサイズ機能そのもの = 別フェーズ（7D は「リサイズが将来実装されたら検知」フックのみ）
- 解像度変更時の画面外クランプ = 当面無視
- 未知 `PanelKind` バリアントの後方互換 = `serde(other)` ではなく warn ログ + 無視

## Phase 7.6 との分担

| 観点 | Phase 7.6 | Phase 7.7 |
|---|---|---|
| 保存トリガー | 手動 (Alt+S/A) + AppExit | 上記 + 自動デバウンス |
| Load トリガー | 手動 (Alt+O) | 上記 + 起動時自動 |
| サイドカーパスの解決 | `original_path.with_extension("json")` | 同左（共有） |
| 「どの `.py` を開くか」の記憶 | なし（毎回 Open Strategy） | `app_state.json` で永続化 |
| viewport データ | スキーマ枠は確保済、保存値はゼロ | 実値を camera から収集 |
| selected_symbol データ | スキーマから除外 | スキーマに追加（後述） |

## 設計

### 7A. グローバル app-state

**場所**: `dirs::config_dir()/the-trader-was-replaced/app_state.json`
（Windows: `%APPDATA%\the-trader-was-replaced\app_state.json`）

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct AppState {
    pub schema_version: u32,
    pub last_strategy_path: Option<PathBuf>,
}
```

- `default_layout` フィールドは **設けない**（`.py` 未開時のフォールバックは hard-coded default にする — Resource 上のデフォルト値で十分）
- 新規 [src/ui/app_state.rs](src/ui/app_state.rs) に I/O ヘルパー
- `OpenStrategyRequested` ハンドラ後に `last_strategy_path` を更新して save

### 7B. 起動時オートオープン

`Startup` system:

```rust
fn restore_last_strategy_system(
    mut events: EventWriter<OpenStrategyRequested>,
) {
    let Ok(state) = load_app_state() else { return };
    let Some(path) = state.last_strategy_path else { return };
    if !path.exists() {
        warn!("last strategy path no longer exists: {:?}", path);
        return;
    }
    info!("restoring last strategy: {:?}", path);
    events.send(OpenStrategyRequested { path });
}
```

`OpenStrategyRequested` 受信側 ([menu_bar.rs](src/ui/menu_bar.rs) `open_strategy_buffer_system`) で **panel 自動 spawn** が必要なら `PanelSpawnRequested` も連鎖発火する。

### 7C. Open 直後の自動 Load

`open_strategy_buffer_system` を拡張、もしくは Update system を追加:

```rust
fn auto_load_sidecar_system(
    mut open_events: EventReader<OpenStrategyRequested>,
    mut load_events: EventWriter<LayoutLoadRequested>,
) {
    for event in open_events.read() {
        let sidecar = event.path.with_extension("json");
        if sidecar.exists() {
            load_events.send(LayoutLoadRequested { path: sidecar });
        }
    }
}
```

**順序問題**: panel が spawn される前に `apply_layout_system` が走ると entity が無くて何もできない。対策:

- (a) `LayoutLoadRequested` を 1 フレーム遅延（`PendingStrategyLoad` と同パターン）
- (b) `apply_layout_system` 側で「未 spawn の kind は次フレームに持ち越す」キューを持つ
- 採用: **(a)** — 既存 `PendingStrategyLoad` の遅延機構と同じ発想で `PendingLayoutLoad` resource を追加

### 7D. 自動デバウンス保存

```rust
#[derive(Resource, Default)]
struct AutoSaveState {
    dirty: bool,
    last_change: Option<Instant>,
}

fn mark_dirty_on_drag_system(
    drag_end: EventReader<Pointer<DragEnd>>, // or window's z-change observer
    mut state: ResMut<AutoSaveState>,
) { ... }

fn debounced_autosave_system(
    mut state: ResMut<AutoSaveState>,
    buffer: Res<StrategyBuffer>,
    panels: Query<...>, camera: Query<...>,
) {
    if !state.dirty { return; }
    let Some(t) = state.last_change else { return };
    if t.elapsed() < Duration::from_secs(1) { return; }
    let Some(py) = buffer.original_path.as_ref() else { return };
    save_layout_to(&py.with_extension("json"), &build_layout(&panels, &camera));
    state.dirty = false;
    state.last_change = None;
}
```

検知対象:
- `Pointer<DragEnd>` on `WindowRoot` / `TitleBar`（位置変化）
- `WindowManager.max_z` 変化（z-order 変化） — `Res::is_changed()` で検知
- `Visibility` 変化 — `Changed<Visibility>` フィルタ
- カメラ `Transform`/`OrthographicProjection.scale` 変化（7E と統合）

### 7E. viewport の収集と適用

7.6 で **スキーマ枠は既に確保済** だが、`build_layout` ではゼロ埋めしていた。本フェーズで実値を入れる:

```rust
fn build_layout(...) -> SidecarLayout {
    let viewport = camera.get_single()
        .map(|(t, p)| ViewportState {
            pan_x: t.translation.x,
            pan_y: t.translation.y,
            zoom: p.scale,
        })
        .unwrap_or_default();
    ...
}
```

`apply_layout_system` 側でも `camera.translation.{x,y}` と `projection.scale` に書き込み。

**PanCam 競合確認** — [src/camera.rs](src/camera.rs) の PanCam 設定で外部 Transform 書き込みを許容するか要検証。`bevy-engine` skill で API 確認。

### 7F. selected_symbol

7.6 でスキーマから除外したフィールドを復活:

```rust
pub struct SidecarLayout {
    pub schema_version: u32,
    pub viewport: ViewportState,
    pub selected_symbol: Option<String>,  // ← 7F で追加
    pub windows: Vec<WindowLayout>,
}
```

- `#[serde(default)]` を付けることで 7.6 が書き出した古い JSON（このフィールドなし）を読める
- サイドバーで銘柄選択した時 `dirty = true`、起動時にサイドバー側へ反映
- 収集経路: サイドバー Resource または `SelectedSymbol` component を grep で探す

**スキーマバージョン**: 7.6 で `SCHEMA_VERSION = 1`、本フェーズでは **同じ 1 のまま** にする（追加フィールドのみで破壊的変更なし、`serde(default)` で互換）。`apply_layout_system` の version チェックは `<=` 比較に変更しておく方が安全。

## 落とし穴・要確認事項

1. **7.6 の AppExit observer と 7D の autosave の競合** — 終了時に AppExit 経路と「dirty を見て save」経路が二重発火する可能性。AppExit 側で `dirty` をクリアする、または autosave 側を `On<AppExit>` 前に確実に走らせる。
2. **`app_state.json` の保存タイミング** — `OpenStrategyRequested` ごとに同期 write するか、AppExit でまとめるか。提案: 同期 write（軽量、クラッシュ耐性高い）。
3. **panel spawn 順序の保証** — `restore_last_strategy_system` (Startup) → `open_strategy_buffer_system` (Update) → `auto_load_sidecar_system` (Update) → `panel_spawn_dispatcher_system` (Update) → `apply_layout_system` (Update)。同フレーム内に並ぶので 1 フレーム遅延が最低 1 回必要。
4. **PanCam の "scale 上書き" 不能問題** — bevy_pancam が `OrthographicProjection.scale` を毎フレーム自前で上書きする場合、外部から書いても次フレームで戻る。要 API 調査。
5. **未知 PanelKind の前方互換** — 将来 7番目の panel が増えた古い JSON を読むと、新 panel の entry がない → そのまま無視（warn）で OK。

## 修正/新規ファイル

- 🆕 [src/ui/app_state.rs](src/ui/app_state.rs) — `AppState` schema + I/O + startup system + update-on-open hook
- ✏️ [src/ui/layout_persistence.rs](src/ui/layout_persistence.rs) — `selected_symbol` 追加、`build_layout` で viewport 実値収集、autosave system + dirty 検知、`apply_layout_system` で viewport/symbol 反映
- ✏️ [src/ui/menu_bar.rs](src/ui/menu_bar.rs) — `open_strategy_buffer_system` で `last_strategy_path` 永続化フック
- ✏️ [src/ui/sidebar.rs](src/ui/sidebar.rs) — selected_symbol 取得・復元フック
- ✏️ [src/camera.rs](src/camera.rs) — PanCam 連動（必要に応じて）
- ✏️ [src/ui/mod.rs](src/ui/mod.rs) — `app_state` module + plugin 拡張

## 検証

1. `cargo test` — 既存 7.6 round-trip テストが新スキーマで通る（後方互換）+ 新規 `app_state.json` round-trip
2. **手動 E2E (オートオープン)**:
   1. `cargo run` → Open Strategy で `foo.py` を開く → 終了
   2. `cargo run` → 自動的に `foo.py` が開く
3. **手動 E2E (オート Load)**:
   1. `foo.py` の隣に `foo.json`（手動編集 or 7.6 で生成）を置く → `cargo run` → 自動 open + 自動 layout 復元
4. **手動 E2E (オート保存)**:
   1. `foo.py` を開く → パネルをドラッグ → 1 秒待つ → `foo.json` が更新される（`stat` で mtime 確認）
   2. ドラッグ → 終了（< 1 秒）→ AppExit 経路で取りこぼしなく保存される
5. **手動 E2E (viewport)**:
   1. カメラを pan/zoom → 終了 → 再起動 → カメラ位置復元
6. **手動 E2E (selected_symbol)**:
   1. サイドバーで銘柄選択 → 終了 → 再起動 → 同銘柄が選択された状態
7. **エッジケース**:
   - `last_strategy_path` のファイルが消えている → warn ログ、起動継続（空状態）
   - `app_state.json` 自体が壊れている → warn、デフォルトで起動
   - サイドカー JSON が壊れている → warn、layout は適用せず panel デフォルト

## 完了条件

「アプリを閉じて再度開いたとき、前回終了時の状態（開いていた `.py`、各 panel の位置/サイズ/可視性、カメラ pan/zoom、選択銘柄）が完全に復元される」をユーザが体感できる。
