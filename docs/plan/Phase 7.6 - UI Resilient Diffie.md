# Phase 7.6 — UI Layout Persistence: 手動 Save/Load + AppExit 自動保存 (Step 2 + Step 3 + Step 4 + メニュー UI)

## Context

[docs/plan/Phase 7.7 - UI Layout Persistence.md](docs/plan/Phase%207.7%20-%20UI%20Layout%20Persistence.md) で計画した永続化機能（マスタープラン）を、**手動 Save / Save As / Load** メニュー操作と **AppExit 自動保存** の両方で実装する。本フェーズは 7.7 のサブセット。

ユーザ判断 (2026-05-15 確認):
- メニューバーに **Save** / **Save As** / **Load** を追加（既存 "Open Strategy..." ボタンと横並び）
- ショートカットは **Ctrl+S / Ctrl+Shift+S / Ctrl+O**（後述の実装変更を参照）
- 保存先デフォルト = 元 `.py` の隣 `<name>.json`、Save As ではダイアログで変更可
- 保存トリガー = **AppExit 自動 + 手動 Save** の併用
- Load を含めないとセーブ内容の整合性が確認できないため、計画書では「対象外」だった **Step 3 (apply_layout system)** も今フェーズに含める

## スコープ

| Step | 内容 | 含む |
|---|---|---|
| 2 | `SidecarLayout` JSON スキーマ（serde 構造体 + round-trip テスト） | ✅ |
| 3 | apply_layout system（Load 用：JSON → 既存 entity の Transform/Sprite/Visibility 上書き） | ✅ |
| 3b | apply_layout: JSON に**ない**パネルを despawn、JSON に**ある**が未 spawn のパネルを re-spawn | ⚠️ **実装中** — despawn は完了、re-spawn が未実装 |
| 4 | AppExit observer による自動保存 | ✅ |
| 追加 | Save / Save As / Load メニューボタン | ✅ |
| 追加 | Ctrl+S / Ctrl+Shift+S / Ctrl+O キーボードショートカット | ✅ |
| 1 | `app_state.json` で前回の `.py` を覚える | ❌ 別フェーズ |
| 5 | ドラッグ中の差分検知 + デバウンス保存 | ❌ 別フェーズ |
| 6 | 起動時の自動 Load | ❌ 別フェーズ（手動 Load のみ） |

## 重要な前提（コード確認済 2026-05-15）

- **`PanelKind` は既に全 6 panel root に attach 済み** — grep で確認:
  - [buying_power.rs:39](src/ui/buying_power.rs), [run_result_panel.rs:41](src/ui/run_result_panel.rs), [positions.rs:61](src/ui/positions.rs), [orders.rs:64](src/ui/orders.rs), [window.rs:22](src/ui/window.rs), [strategy_editor.rs:114](src/ui/strategy_editor.rs) すべて `commands.entity(root).insert(PanelKind::*)` 済み
  - → 旧計画書で危惧していた「spawn 6箇所への PanelKind 追加」は **不要**
- `WindowRoot` は [floating_window.rs:45](src/ui/floating_window.rs#L45) で attach
- `WindowManager.max_z: f32`（`i32` ではない） — z-order スキーマは `f32` で持つのが素直
- Sprite サイズは [floating_window.rs:43](src/ui/floating_window.rs#L43) で `custom_size: Some(spec.size)`（root sprite 自体は border 込みではない）— root sprite の `custom_size` をそのまま保存可

## 設計

### 1. サイドカー JSON スキーマ (Step 2)

新規 [src/ui/layout_persistence.rs](src/ui/layout_persistence.rs):

```rust
use crate::ui::components::PanelKind;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SidecarLayout {
    pub schema_version: u32,
    pub viewport: ViewportState,
    pub windows: Vec<WindowLayout>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewportState {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WindowLayout {
    pub kind: PanelKind,
    pub visible: bool,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub z: f32,
}
```

- [PanelKind](src/ui/components.rs#L113-L135) に `#[derive(Serialize, Deserialize)]` を追加
- 単体テスト: `SidecarLayout` の round-trip（`to_string_pretty` → `from_str` で eq）
- `selected_symbol` は今フェーズでは収集経路がないため **スキーマから除外**（将来追加時に `#[serde(default)]` で後方互換可）

### 2. ファイル I/O 関数

```rust
fn build_layout(
    panels: &Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: &Query<(&Transform, &OrthographicProjection), With<Camera2d>>,
) -> SidecarLayout { ... }

fn save_layout_to(path: &Path, layout: &SidecarLayout) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(layout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}

fn load_layout_from(path: &Path) -> std::io::Result<SidecarLayout> {
    let json = std::fs::read_to_string(path)?;
    serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
```

### 3. apply_layout system (Step 3 — Load 用) ⚠️ re-spawn が未実装

現在の実装（`src/ui/layout_persistence.rs`）:

```rust
fn apply_layout_system(
    mut commands: Commands,
    mut events: EventReader<LayoutLoadRequested>,
    mut panels: Query<(Entity, &PanelKind, &mut Transform, &mut Sprite, &mut Visibility), With<WindowRoot>>,
    mut camera: Query<(&mut Transform, &mut OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    mut wm: ResMut<WindowManager>,
) {
    for event in events.read() {
        // 1. JSON 読み込み + schema_version チェック
        // 2. viewport を camera に反映
        // 3. 各 WindowLayout を kind でマッチして既存 entity に上書き
        // 4. JSON に掲載されていない entity を despawn_recursive ← ✅ 実装済み
        // 5. JSON に掲載されているが ECS に存在しない kind を re-spawn ← ⚠️ 未実装
    }
}
```

#### ✅ 実装済み: JSON にないパネルを despawn

```rust
let to_despawn: Vec<Entity> = panels
    .iter()
    .filter(|(_, kind, _, _, _)| !layout.windows.iter().any(|w| w.kind == **kind))
    .map(|(entity, _, _, _, _)| entity)
    .collect();
for entity in to_despawn {
    commands.entity(entity).despawn_recursive();
}
```

#### ⚠️ 未実装: JSON にある PanelKind が ECS に存在しない場合の re-spawn

**次の作業者がここを実装すること。**

設計方針:
- `apply_layout_system` の `find` で `None` になったとき（現在は `warn` してスキップ）、
  代わりに対応する spawn 関数を呼んで entity を作り直す
- spawn 後に position / size / visibility を即座に適用する
- 各 PanelKind の spawn 関数を調べ、`apply_layout_system` から呼べる形に整理する

実装する前に以下のファイルを Read して spawn 関数のシグネチャを把握すること:
- `src/ui/floating_window.rs` — `FloatingWindowSpec` / `spawn_floating_window` の構造
- `src/ui/window.rs` — Chart パネルの spawn
- `src/ui/orders.rs` — Orders パネルの spawn
- `src/ui/positions.rs` — Positions パネルの spawn
- `src/ui/buying_power.rs` — BuyingPower パネルの spawn
- `src/ui/run_result_panel.rs` — RunResult パネルの spawn
- `src/ui/strategy_editor.rs` — StrategyEditor パネルの spawn

### 4. AppExit 自動保存 ✅ 実装済み（EventReader 方式）

> **注意**: 設計書の `Trigger<AppExit>` + `add_observer` は **発火しない**（後述の Bevy 0.15 落とし穴を参照）。
> 実装は `EventReader<WindowCloseRequested>` + `add_systems(Update, ...)` に変更済み。

```rust
fn save_layout_on_window_close(
    mut close_events: EventReader<WindowCloseRequested>,
    panels: Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: Res<StrategyBuffer>,
) {
    for _ in close_events.read() {
        let Some(orig) = &buffer.original_path else {
            info!("layout auto-save skipped: no original_path");
            continue;
        };
        // ...
    }
}
```

### 5. メニューボタン (新規) ✅ 実装済み

[src/ui/components.rs](src/ui/components.rs) の `MenuButton` enum を拡張:

```rust
pub enum MenuButton {
    OpenStrategy,
    SaveLayout,
    SaveLayoutAs,
    LoadLayout,
}
```

[src/ui/menu_bar.rs](src/ui/menu_bar.rs) の `spawn_menu_bar` に3ボタン追加（"Open Strategy..." の右隣、"flex_grow: 1.0" スペーサーの前）:

```rust
spawn_menu_btn(p, "Save (Ctrl+S)",        MenuButton::SaveLayout);
spawn_menu_btn(p, "Save As (Ctrl+Shift+S)", MenuButton::SaveLayoutAs);
spawn_menu_btn(p, "Load (Ctrl+O)",        MenuButton::LoadLayout);
```

`menu_button_system` を拡張し、各 action で対応するイベント（`LayoutSaveRequested`, `LayoutSaveAsRequested`, `LayoutLoadDialogRequested`）を発火。Save As / Load は `rfd::FileDialog` を使う（既存 OpenStrategy パターン踏襲）。

### 6. キーボードショートカット ✅ 実装済み（Alt → Ctrl に変更済み）

**当初計画は Alt+S/A/O だったが E2E 検証で問題が判明し Ctrl 系に変更した。**

> **問題の経緯**:
> Alt+S を押すと `bevy_cosmic_edit::input::kb_input_text` が `S` を文字入力として処理し、
> `cosmic-text 0.12.1` の `buffer_line.rs:158` で `assert!(self.is_char_boundary(at))` パニック。
> `FocusedWidget` ガードで回避を試みたが、ガード時でも `S` がエディタに書き込まれる UX 問題が残った。
> 根本原因: Alt combo は cosmic-edit がテキスト入力として扱う。Ctrl combo は扱わない。

実装済みのショートカット:
- **Ctrl+S** → `LayoutSaveRequested`
- **Ctrl+Shift+S** → `LayoutSaveAsRequested`
- **Ctrl+O** → `LayoutLoadDialogRequested`

キーリピート多重発火対策として `Local<f32>` + 500ms クールダウンを実装済み。

### 7. Save パスの解決 ✅ 実装済み

`LayoutSaveRequested` ハンドラ内:
- `buffer.original_path` が `Some(py)` → `py.with_extension("json")`（同ディレクトリ・同名）
- `None` → ダイアログを開く（SaveAs にフォールバック）

### 8. 起動時 Load・自動保存は **しない**

今フェーズでは:
- Load は明示操作（メニュー or Ctrl+O）のみ
- 保存は明示操作（メニュー or Ctrl+S/Ctrl+Shift+S）と AppExit のみ

## Bevy 0.15 API 確認事項（実装済み・知見あり）

- `ButtonInput<KeyCode>` — 問題なし
- `OrthographicProjection.scale` への書き込みで PanCam 側との競合なし確認

### ⚠️ Bevy 0.15 落とし穴: `app.add_observer()` vs `app.observe()`

**`app.observe(system)` は Bevy 0.15 に存在しない。`app.add_observer(system)` を使う。**

- エンティティローカルの `.observe(...)` は引き続き使える（`commands.entity(e).observe(...)`）
- `Plugin::build` 内でグローバル observer を登録するときは必ず `app.add_observer(...)` を使うこと

### ⚠️ Bevy 0.15 落とし穴: AppExit / WindowCloseRequested は EventWriter 経由

**`app.add_observer(fn)` で `Trigger<AppExit>` や `Trigger<WindowCloseRequested>` を受け取る observer は発火しない。**

- Bevy 0.15 の winit backend は `AppExit`・`WindowCloseRequested` を `EventWriter::send()` で送る
- `app.add_observer` が期待するのは `commands.trigger_targets()` 経由の triggered event
- この 2 つは異なる経路のため、observer は一切呼ばれない

**正しいアプローチ**: `EventReader<WindowCloseRequested>` を受け取る通常 system + `add_systems(Update, ...)` を使う。`WindowCloseRequested` は user が×ボタンを押した frame で送られ、window entity がまだ生きているうちに受信できる。

**調査の経緯（2026-05-15）**:
1. 当初 `Trigger<AppExit>` + `add_observer` → 発火しない
2. `Trigger<WindowCloseRequested>` + `add_observer` → これも発火しない
3. 根本原因: winit backend が `EventWriter::send()` を使用
4. `EventReader<WindowCloseRequested>` + `add_systems(Update, ...)` に変更 → ✅ 解決

### ⚠️ bevy_cosmic_edit のパニック（終了時）

アプリ終了時に `bevy_cosmic_edit::utils::change_active_editor_sprite` が
`NoEntities(PrimaryWindow)` でパニックする。bevy_cosmic_edit 側のバグ。
セーブは `WindowCloseRequested` 受信時（window entity がまだ存在する frame）に完了するため実害なし。
Phase 7.6 のスコープ外として放置。

## DPI トラップ（既存メモリ）

- `Transform.translation` は world-space → DPI 非依存 → そのまま保存 OK
- `Sprite.custom_size` も論理サイズ → そのまま OK
- `cosmic-edit Buffer メトリクスの DPI トラップ` は今フェーズに **無関係**

## 修正/新規ファイル

- 🆕 [src/ui/layout_persistence.rs](src/ui/layout_persistence.rs) — schema + I/O + auto-save + apply_system + shortcut_system + plugin + tests
- ✏️ [src/ui/components.rs](src/ui/components.rs) — `PanelKind` に `Serialize/Deserialize` derive、`MenuButton` に 3 variant 追加
- ✏️ [src/ui/menu_bar.rs](src/ui/menu_bar.rs) — `spawn_menu_bar` に 3 ボタン追加、`menu_button_system` を 4 action 対応に拡張
- ✏️ [src/ui/mod.rs](src/ui/mod.rs) — `pub mod layout_persistence;`、`UiPlugin` に `LayoutPersistencePlugin` 登録
- ❌ spawn 関数 6 箇所修正 — **不要**（PanelKind は既に attach 済み）

## 検証

1. ✅ `cargo test --lib sidecar_layout_round_trip` — pass（1 test ok）
2. ✅ `cargo build` — pass
3. ✅ **手動 E2E（2026-05-15 完了）**:
   1. ✅ Scenario A: Ctrl+S で JSON 保存 → Ctrl+O で位置復元 — **PASSED**
   2. ✅ Scenario B: Ctrl+Shift+S で Save As ダイアログ → 保存 — **PASSED**
   3. ✅ Scenario C: ×ボタンで閉じる → `layout auto-saved to ...json` 発火 — **PASSED**
   4. ✅ Scenario D: `.py` 未オープンで Ctrl+S → ダイアログ fallback → 保存/キャンセル両方正常 — **PASSED**
   5. ✅ JSON にないパネルが Load 後に despawn される — **PASSED**
4. **未確認項目**:
   - schema_version 不一致 JSON を Load → warn してスキップ（実装済み・テスト未実施）
   - 未 spawn の PanelKind を含む JSON を Load → **re-spawn（実装待ち）**

### 次の作業者への引継ぎ事項

**残タスク 1 件: `apply_layout_system` の re-spawn 実装**

現状:
- JSON に存在するが ECS に entity がない PanelKind は `warn` してスキップされる
- ユーザー要求: Load 時に JSON にある PanelKind が存在しなければ spawn し直す

実装手順:
1. `src/ui/floating_window.rs` と各パネルの spawn 関数（window.rs, orders.rs, positions.rs, buying_power.rs, run_result_panel.rs, strategy_editor.rs）を Read して spawn のシグネチャを把握
2. `apply_layout_system` の `None =>` アーム（現在 `warn!` してスキップ）で spawn を呼ぶ
3. spawn 直後に `WindowLayout` の position / size / visible / z を適用する
4. `cargo build` + 手動 E2E で確認

**既存の知見・落とし穴**（上記「Bevy 0.15 API 確認事項」を必ず読むこと）:
- `EventReader<WindowCloseRequested>` + `add_systems(Update, ...)` — 触らない
- despawn ロジック（`to_despawn` Vec → `despawn_recursive`）— 触らない
- キーリピート 500ms クールダウン — 触らない

**保存先パス**:
`buffer.original_path` が `Some(path)` のとき → `path.with_extension("json")`（同ディレクトリ・同名）
例: `python/tests/data/foo.py` → `python/tests/data/foo.json`

## 次フェーズ（**対象外** — すべて [Phase 7.7](Phase%207.7%20-%20UI%20Layout%20Persistence.md) で扱う）

| 7.7 サブステップ | 内容 |
|---|---|
| 7A | `app_state.json` で前回の `.py` を永続化 |
| 7B | 起動時に `last_strategy_path` を自動 open |
| 7C | Open 直後にサイドカー `.json` を自動 Load |
| 7D | ドラッグ/リサイズ後の 1 秒デバウンス自動保存 |
| 7E | viewport (camera pan/zoom) の保存・復元 |
| 7F | `selected_symbol` の保存・復元（スキーマ拡張あり） |

本フェーズ (7.6) では **手動操作 (Ctrl+S/Ctrl+Shift+S/Ctrl+O) と AppExit のみ**。「アプリを開き直したら前回の続きから始まる」体験は 7.7 で完成する。
