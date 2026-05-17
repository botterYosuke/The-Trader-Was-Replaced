# Instruments Add → Menu風 Dropdown 化

## Context

現状、Sidebar の `Instruments` セクション内 `[+ Add]` ボタンを押すと、
**world-space の floating window**（タイトル "Add Instrument"・360x480px・Sprite + Text2d ベース）が
別ウィンドウとして spawn され、銘柄一覧と検索ボックスが表示される
（`src/ui/instrument_picker.rs` の `spawn_picker_window` ほか）。

ユーザー要望: これを `src/ui/menu_bar.rs` の File / Edit ▾ と同じ
**UI Node ベースの dropdown** に置き換え、`[+ Add]` ボタンの**右側**に出す。
（menu は下に開くが、Add ボタンは Sidebar 内なので右に出すのが自然。）

利点:
- 既存 sidebar が UI Node (Bevy UI) で組まれているので、dropdown も同じ流派にできる。
  座標系・hit test・z-order が UI 階層で完結し、PanCam ズームの影響を受けない。
- menu_bar の `OpenMenu` / `Display::None ↔ Flex` パターンに揃えると挙動が一貫する。
- 不可視中の Sprite / Text2d を相手にしない分、画面外でのフレームコストが減る。

決定事項（事前ヒアリング）:
1. **検索ボックスは残す**（dropdown 上部に Text 入力、下部に行リスト）。
2. **旧 world-space picker は完全削除**（`spawn_picker_window` 等の Text2d/Sprite 経路）。

## Approach

1. `[+ Add]` ボタン (Node) の **子として**、`menu_bar.rs` の File popup と同じ手法で
   `Node { display: Display::None, position_type: Absolute, left: Val::Percent(100.0), top: Val::Px(0.0), ... }`
   を持つ popup ノードを spawn する。`InstrumentPickerState.visible` の値に従って
   `Display::Flex / None` をトグルする system を 1 本足す。
   **trigger 条件は `picker.is_changed() || !added_dropdown_q.is_empty()` の OR**
   にする — `update_sidebar_system` が registry 変更で sidebar list descendants を
   作り直す（`sidebar.rs:160` の `despawn_descendants`）たびに dropdown も再 spawn される
   ため、`is_changed()` だけだと再 spawn 直後の dropdown が `Display::None` のまま残る。
   menu_bar の `sync_menu_popup_visibility_system` をそのまま踏襲すると同じ罠を踏むので
   注意（menu_bar は popup が startup で 1 回 spawn されるだけだから問題が顕在化しない）。
2. popup の中身は **Bevy UI Node** で:
   - 上段: `Text` (現 query) を背景色付き `Node` で表示（検索結果は `picker.query` を SoT）。
   - 下段: `InstrumentPickerListContainer`（UI Node 版・新規 marker）に Button 行を生成。
3. 行 Button は `Interaction::Pressed` で `handle_picker_row_click` を呼ぶ既存ロジックを再利用。
   現在は `observe(Pointer<Down>)` で実装されているが、UI Node では Bevy 標準の
   `Interaction` を見る system 1 本に置き換える（observer は world-space sprite 用なので不要）。
4. **state / data flow は維持**:
   - `InstrumentPickerState` resource（visible / end_date / query / last_added 等）はそのまま。
   - `AvailableInstruments` の fetch 経路（`add_instrument_button_system` 内の
     debounce + `TransportCommand::FetchAvailableInstruments`）は触らない。
   - `picker_searchbox_input_system` の **キー入力ロジックも維持**（Esc で close、
     文字で query 更新、`kb_events.clear()` で cosmic_edit への二重配送防止）。
     書き込み先だけ Text2d → UI `Text` に変える。
5. **旧 world-space picker の削除**:
   - `spawn_picker_window` / `InstrumentPickerWindow` / `InstrumentPickerSearchBox` (Text2d 版)
     / `spawn_picker_row` / `picker_close_when_invisible_system` /
     `picker_sync_visible_on_window_removed_system` を削除し、それぞれを参照する
     `mod.rs` の `add_systems` 登録と `use` も外す。
   - `picker_list_rebuild_system` は **UI Node 版に書き換え**（despawn → Node Button 行を
     新コンテナに spawn）。Loading / Error / "No matches" / placeholder の 4 状態は同じ
     ラベルで踏襲する。
6. **Sidebar の overflow**:
   - `spawn_sidebar` (`src/ui/mod.rs` の `SidebarRoot`) は `overflow: Overflow::clip_y()`。
     これだと右側に開く popup は左右はクリップされないので OK。ただし `SidebarInstrumentsList`
     や Add ボタンを含む Node に幅で押さえつけるスタイルが無いか確認し、必要なら
     `overflow: Overflow::visible()` を Add ボタンの祖先に明示する。

## Critical files to modify

- [src/ui/instrument_picker.rs](src/ui/instrument_picker.rs:1-784) — メインの書き換え対象。
  - 削除: `spawn_picker_window` (72-113), `InstrumentPickerWindow` /
    `InstrumentPickerSearchBox` の Text2d/Sprite 構築, `spawn_picker_row` (328-390),
    `picker_close_when_invisible_system` (212-223),
    `picker_sync_visible_on_window_removed_system` (235-244).
  - 追加: `spawn_picker_dropdown(parent: &mut ChildBuilder)` ヘルパ — Add ボタンの
    `with_children` 内から呼んで popup Node を生成（display=None で start）。
  - 追加: `sync_picker_dropdown_visibility_system` — `picker.visible` 変化を Node display に反映。
  - 書き換え: `picker_list_rebuild_system` (414-513) — UI Node ベース。
    container は `InstrumentPickerListContainer` (UI Node 版) を使い、行は
    `Button` + `Text`。click は `Interaction::Pressed` を読む新規 system
    `picker_row_click_system` に分離する。
  - 書き換え: `picker_searchbox_input_system` (269-322) — `Text2d` 書き込みを
    `Text` (UI) 書き込みに差し替え。ロジック (drain → push/pop/Esc) はそのまま。
  - `add_instrument_button_system` (135-205) — `spawn_picker_window` 呼び出しを削除し、
    **トグル動作**にする: `was_visible == true` なら `picker.visible = false` にして即 return
    （fetch / query reset を走らせない）。`was_visible == false` のときだけ既存の
    end_date snapshot + query.clear + fetch dispatch ロジックを通す。dropdown は事前に
    Add ボタン直下に spawn 済みなので、visibility sync system が `picker.visible` の
    両方向変化を拾う。これで「`[+ Add]` 再押下で閉じる」検証要件と整合する。

- [src/ui/sidebar.rs](src/ui/sidebar.rs:243-268) — `update_sidebar_system` 内の
  `[+ Add]` Button spawn 部分で、`.with_children(|btn| { ... })` に `spawn_picker_dropdown`
  呼び出しを追加。Add ボタンの `Node` に `position_type: PositionType::Relative` を確実に付け、
  `overflow: Overflow::visible()` を Add ボタン Node に追加する（祖先 `SidebarInstrumentsList`
  や `SidebarRoot` は clip_y のままで OK; 左右はデフォルトで visible）。

- [src/ui/mod.rs](src/ui/mod.rs:45-49) — `use` から削除する関数名を整理し、
  `add_systems` 登録から `picker_close_when_invisible_system` /
  `picker_sync_visible_on_window_removed_system` を外す。新規
  `sync_picker_dropdown_visibility_system` と `picker_row_click_system` を追加。

- **Marker component の線引き**（`instrument_picker.rs` 内に集約。`components.rs` への
  追加は不要）:
  - **削除する** (world-space 専用):
    - `InstrumentPickerWindow` — floating window root marker。spawn 経路と
      `picker_sync_visible_on_window_removed_system` も含めて削除。
    - `InstrumentPickerSearchBox` — Text2d 用の searchbox marker。UI Node 版の
      新規 marker `InstrumentPickerSearchText` に置換。
  - **UI Node 版として再利用 (型はそのまま、spawn 文脈だけ変える)**:
    - `InstrumentPickerListContainer` — 行 Button を子に持つ Node の marker。
    - `InstrumentPickerRow` — 1 行に貼るデータ marker (`instrument_id`, `already_added`)。
    - `InstrumentPickerAddButton` — クリック対象 Button の marker。
      新規 `picker_row_click_system` が `Query<&Interaction, With<InstrumentPickerAddButton>>` で読む。
  - **新規追加**:
    - `InstrumentPickerDropdown` — popup Node 自身の marker。visibility sync 用。
    - `InstrumentPickerSearchText` — 検索クエリを表示する `Text` (UI) の marker。

## Reused existing utilities

- **`menu_bar.rs` の popup 階層パターン** (lines 91-109 の File popup)
  — `display`, `position_type: Absolute`, child as button-of-button 配置を踏襲。
- **`InstrumentPickerState` resource** — visible / query / debounce 状態。SoT として継続使用。
- **`handle_picker_row_click`** (`instrument_picker.rs:394-410`) — 純粋ハンドラ。
  click 経路を Pointer から Interaction に変えても再利用可能（既存テスト群もそのまま通る）。
- **`add_instrument_button_system`** の fetch debounce / `BackendStatus` チェックロジック
  (lines 168-203) — まったく変更しない。
- **`picker_searchbox_input_system`** のキー drain + `kb_events.clear()` パターン
  — `menu_keyboard_system` と並ぶ二重配送防止の確立済みパターン。

## Verification

1. `cargo check` がクリーンに通ること（特に `mod.rs` の use 整理ミスを検出）。
2. `cargo test -p backcast --lib instrument_picker`（crate 名は [Cargo.toml](Cargo.toml) の `name = "backcast"`）—
   既存ユニットテストのうち `handle_picker_row_click` 系（テスト 5-a-2）は無変更で通る。
   `test_picker_opens_on_add_button_pressed` / `test_picker_skips_open_when_registry_locked` /
   `test_picker_skips_open_during_debounce` / `test_picker_sets_last_error_when_backend_disconnected`
   は `add_instrument_button_system` の `spawn_picker_window` 呼び出しが消えたので
   `app.update()` で window entity が無くてもよい想定に直す（assertion は
   `picker.visible` と `available.in_flight` / `last_error` のみで成立しているため軽微）。
   `test_picker_list_rebuilds_on_reopen_after_close` は dropdown 版に書き換え:
   `InstrumentPickerWindow` 数の検査を `InstrumentPickerDropdown` の `Display` 検査に置換。
3. `cargo run` 起動 → backend 接続後に Sidebar `[+ Add]` をクリック:
   - Add ボタンの右に dropdown が出る。
   - 文字を打つと先頭行の検索ボックス Text が更新され、下の行がフィルタされる。
   - 行をクリックすると Instruments リストに追加され、dropdown は閉じない（連続 add）。
   - もう一度 `[+ Add]` を押す or Esc キーで dropdown が閉じる。
   - dropdown 開中に Sidebar をスクロール / リサイズしても popup が Add ボタンに追随する
     （`position_type: Absolute, left: 100%` で確認）。
4. backend 切断時に `[+ Add]` を押すと dropdown 内に `Error: backend not connected`
   行が 1 行だけ出る（無限 Loading にならない）。
5. `registry.editable=false`（`instruments_ref` sidecar）時は `[+ Add]` 押下で
   dropdown が出ない／開いている dropdown が `force_close_picker_on_lock_system` で
   閉じる、を目視確認。

## Out of scope

- スクロール対応（リストは現状の `take(15)` のままで打ち止め）。フィルタが検索で吸収できる。
- dropdown 外クリックでの自動 close — menu_bar 自体が未実装なので踏襲。Esc または
  `[+ Add]` 再押下で閉じる。必要になれば後続フェーズで `OpenMenu` 風の独立
  resource を追加する余地あり。
- styling の凝った装飾（rim light / accent）は持ち込まない。menu popup と同じ
  `BackgroundColor(Color::srgba(0.10, 0.10, 0.16, 0.98))` + `GlobalZIndex(100)` で揃える。
