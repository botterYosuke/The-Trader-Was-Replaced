//! J2 strategy_editor_tab_indent — Strategy Editor で Tab を押すと tab 文字ではなく
//! 4 スペースが挿入されることを保証する（kind:ui）。
//!
//! `tab_input_system` は `ButtonInput<KeyCode>` に Tab が `just_pressed` のとき、
//! focused な `StrategyEditorContent` entity の `CosmicEditor` に 4 回スペースを挿入し
//! `CosmicTextChanged` を発行する。本テストはその経路を headless で走らせる。
//!
//! 注: `CosmicEditor` は通常 bevy_cosmic_edit の focus system が `CosmicEditBuffer` から
//! 自動生成するが、headless テストでは `Editor::new(buf.clone())` で手動で構築する。

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, Editor, Metrics};
use bevy_cosmic_edit::prelude::FocusedWidget;
use bevy_cosmic_edit::{CosmicEditBuffer, CosmicEditor, CosmicFontSystem, CosmicTextChanged};
use cosmic_text::{Edit, FontSystem};

use backcast::ui::strategy_editor::StrategyEditorContent;
use backcast::ui::strategy_editor_input::tab_input_system;

#[test]
fn j2_strategy_editor_tab_indent() {
    let mut app = App::new();

    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(14.0, 18.0);

    // "def foo():\n" をシード。Tab はカーソル位置 (末尾) に 4 スペースを挿入する。
    let seed_text = "def foo():";

    // CosmicEditBuffer を作成し、そこから Editor を構築して CosmicEditor に wrap する。
    // これは bevy_cosmic_edit の focus system が行う `Editor::new(b.0.clone())` と同じ手順。
    let buf = CosmicEditBuffer::new(&mut font_system, metrics)
        .with_text(&mut font_system, seed_text, Attrs::new());
    let inner_buf_clone = buf.0.clone();
    let mut editor = Editor::new(inner_buf_clone);
    editor.set_redraw(true);
    let cosmic_editor = CosmicEditor::new(editor);

    app.insert_resource(CosmicFontSystem(font_system))
        .insert_resource(FocusedWidget(None))
        .insert_resource(ButtonInput::<KeyCode>::default())
        .add_event::<CosmicTextChanged>()
        .add_event::<KeyboardInput>()
        .add_systems(Update, tab_input_system);

    // StrategyEditorContent entity (CosmicEditor を持つ editor)。
    let editor_entity = app
        .world_mut()
        .spawn((StrategyEditorContent, buf, cosmic_editor))
        .id();

    // FocusedWidget を editor entity に向ける。
    app.world_mut().resource_mut::<FocusedWidget>().0 = Some(editor_entity);

    // ── Tab キーを just_pressed に設定 ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Tab);
    }
    app.update();

    // ── CosmicTextChanged が発火したか確認 ──
    let changed_events: Vec<(Entity, String)> = app
        .world_mut()
        .resource_mut::<Events<CosmicTextChanged>>()
        .drain()
        .map(|CosmicTextChanged(pair)| pair)
        .collect();

    assert_eq!(
        changed_events.len(),
        1,
        "Tab 入力で CosmicTextChanged が 1 件発火するはず (got {})",
        changed_events.len()
    );

    let new_text = &changed_events[0].1;
    // 4 スペースが挿入されていること (tab 文字 '\t' ではない)。
    assert!(
        !new_text.contains('\t'),
        "Tab は tab 文字ではなくスペースに変換されるはず (text={:?})",
        new_text
    );
    // seed "def foo():" のどこかに 4 スペース分が入っているはず。
    let space_count = new_text.chars().filter(|c| *c == ' ').count();
    assert!(
        space_count >= 4,
        "Tab 1 回で 4 スペースが挿入されるはず (space_count={space_count}, text={:?})",
        new_text
    );

    // Tab キーは reset されているはず (二重発火防止)。
    {
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(
            !keys.just_pressed(KeyCode::Tab),
            "tab_input_system が Tab キーを reset するはず"
        );
    }
}
