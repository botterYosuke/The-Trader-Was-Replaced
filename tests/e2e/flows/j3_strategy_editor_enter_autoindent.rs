//! J3 strategy_editor_enter_autoindent — Strategy Editor で Enter が `InsertNewline` action として
//! bevscode に届くことを保証する（kind:ui / wiring guard）。
//!
//! Iter3 A2 fix: 旧版は `bevscode::input::auto_indent::compute_newline_indent` を直接叩いて
//! bevscode 内部実装の単体テストを我々の repo で重複していたため、`install_strategy_editor_keybindings`
//! が壊れても green になっていた。j2 (Tab/InsertTab) と同じ wiring-guard 形式に揃え、
//! InputMap に InsertNewline=Enter が残っていることを assert する。実際の autoindent 文字列計算は
//! bevscode 側の単体テストでカバーされる。

use backcast::ui::strategy_editor::install_strategy_editor_keybindings;
use bevscode::input::EditorAction;
use bevscode::plugin::EditorInputManager;
use bevy::prelude::*;
use bevscode::input::InputMap;

#[test]
fn j3_strategy_editor_keybindings_have_insert_newline() {
    let mut app = App::new();
    app.add_systems(Startup, install_strategy_editor_keybindings);
    app.update();

    let mut q = app
        .world_mut()
        .query_filtered::<&InputMap<EditorAction>, With<EditorInputManager>>();
    let input_map = q
        .iter(app.world())
        .next()
        .expect("install_strategy_editor_keybindings が EditorInputManager を spawn していない");

    let buttons = input_map.get_buttonlike(&EditorAction::InsertNewline);
    assert!(
        buttons.is_some_and(|v| !v.is_empty()),
        "InsertNewline が bind されていない: {:?}",
        buttons,
    );
    let has_enter = buttons.unwrap().iter().any(|b| format!("{b:?}").contains("Enter"));
    assert!(
        has_enter,
        "InsertNewline の binding に KeyCode::Enter が含まれない: {:?}",
        buttons,
    );
}
