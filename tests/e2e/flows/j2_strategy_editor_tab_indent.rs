//! J2 strategy_editor_tab_indent — Strategy Editor で Tab が `InsertTab` action として
//! bevscode に届くことを保証する（kind:ui）。
//!
//! Slice 3 (#50): cosmic_edit 撤去にあわせて、自前 `tab_input_system` の検証から
//! 「bevscode が Tab を InsertTab にマップした InputMap を持つ」+「我々が install する
//! InputMap が Undo を剥がしつつ InsertTab は残す」という wiring contract に変更。
//! 実際の "Tab → 4 スペース挿入" 動作は bevscode の `input::auto_indent` /
//! `input::actions::should_skip_auto_close` などが担い、bevscode 側の単体テストで
//! `tab_indented_input_yields_tab_indent_string` などが上流コアレッジを与える。
//! ここでは我々の側の「正しい InputMap を spawn しているか」だけを担保する。

use bevscode::input::{EditorAction, InputMap};
use bevy::input::keyboard::KeyCode;

use backcast::ui::strategy_editor::install_strategy_editor_keybindings;
use bevy::prelude::*;
use bevscode::plugin::EditorInputManager;

#[test]
fn j2_strategy_editor_keybindings_have_insert_tab_without_undo() {
    let mut app = App::new();
    app.add_systems(Startup, install_strategy_editor_keybindings);
    app.update();

    // install_strategy_editor_keybindings は Startup で EditorInputManager + 独自 InputMap を spawn する。
    // その entity から InputMap<EditorAction> を取り出し、Tab / Undo / Redo の含有を確認する。
    let mut q = app
        .world_mut()
        .query_filtered::<&InputMap<EditorAction>, With<EditorInputManager>>();
    let input_map = q
        .iter(app.world())
        .next()
        .expect("install_strategy_editor_keybindings が EditorInputManager を spawn していない");

    // Tab → InsertTab が登録されている（bevscode default を継承）
    let buttons_for_tab = input_map.get_buttonlike(&EditorAction::InsertTab);
    assert!(
        buttons_for_tab.is_some_and(|v| !v.is_empty()),
        "InsertTab が Tab に bind されていない: {:?}",
        buttons_for_tab,
    );
    let any_tab = buttons_for_tab.unwrap().iter().any(|b| {
        format!("{b:?}").contains("Tab")
    });
    assert!(
        any_tab,
        "InsertTab の binding に KeyCode::Tab が含まれない: {:?}",
        buttons_for_tab,
    );

    // Undo / Redo は剥がされている（AppHistory が undo/redo を担う設計）
    let undo = input_map.get_buttonlike(&EditorAction::Undo);
    assert!(
        undo.is_none() || undo.unwrap().is_empty(),
        "Undo を剥がし切れていない: {:?} — AppHistory との undo 二重化が起きる",
        undo,
    );
    let redo = input_map.get_buttonlike(&EditorAction::Redo);
    assert!(
        redo.is_none() || redo.unwrap().is_empty(),
        "Redo を剥がし切れていない: {:?}",
        redo,
    );

    // 念のため: 他の編集アクション（InsertNewline）は残っていることだけ軽く確認
    let newline = input_map.get_buttonlike(&EditorAction::InsertNewline);
    assert!(
        newline.is_some_and(|v| !v.is_empty()),
        "InsertNewline まで剥げている: bevscode default が壊れているか install 関数が default_input_map を呼んでいない",
    );

    let _ = KeyCode::Tab; // ensure import is used in cfg gates
}
