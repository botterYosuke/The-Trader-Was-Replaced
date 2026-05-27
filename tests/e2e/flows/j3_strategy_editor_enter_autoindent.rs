//! J3 strategy_editor_enter_autoindent — Strategy Editor で Enter を押すと前行のインデントを
//! 新しい行へ引き継ぐことを保証する（kind:ui）。
//!
//! `enter_autoindent_system` はインデント 4 スペースの行でカーソルが末尾にあるとき Enter を
//! 検知し、改行を挿入してから同じ幅のスペースを先頭に挿入する。
//! 本テストはその経路を headless で走らせ、改行後の新しい行が前行と同じインデントで始まることを
//! assert する。
//!
//! VERIFY 済み: `enter_autoindent_system` は `src/ui/strategy_editor_input.rs` に実装されている。

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, Edit, Editor, Metrics};
use bevy_cosmic_edit::prelude::FocusedWidget;
use bevy_cosmic_edit::{CosmicEditBuffer, CosmicEditor, CosmicFontSystem, CosmicTextChanged};
use cosmic_text::FontSystem;

use backcast::ui::strategy_editor::StrategyEditorContent;
use backcast::ui::strategy_editor_input::enter_autoindent_system;

#[test]
fn j3_strategy_editor_enter_autoindent() {
    let mut app = App::new();

    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(14.0, 18.0);

    // インデント 4 スペースを持つ行（Enter 後に同じインデントを継承するはず）。
    let seed_text = "    x = 1";

    let buf = CosmicEditBuffer::new(&mut font_system, metrics)
        .with_text(&mut font_system, seed_text, Attrs::new());
    let inner_buf_clone = buf.0.clone();
    let mut editor = Editor::new(inner_buf_clone);
    editor.set_redraw(true);
    // enter_autoindent_system は cursor 行の行頭インデントを継承する。前行 (line 0) の
    // インデントを継承させるにはカーソルを行末に置く必要がある (行頭のままだと改行が
    // 行の前に入り、新しい line 0 が空・line 1 が二重インデントになってしまう)。
    editor.action(
        &mut font_system,
        bevy_cosmic_edit::cosmic_text::Action::Motion(
            bevy_cosmic_edit::cosmic_text::Motion::End,
        ),
    );
    let cosmic_editor = CosmicEditor::new(editor);

    app.insert_resource(CosmicFontSystem(font_system))
        .insert_resource(FocusedWidget(None))
        .insert_resource(ButtonInput::<KeyCode>::default())
        .add_message::<CosmicTextChanged>()
        .add_message::<KeyboardInput>()
        .add_systems(Update, enter_autoindent_system);

    let editor_entity = app
        .world_mut()
        .spawn((StrategyEditorContent, buf, cosmic_editor))
        .id();

    app.world_mut().resource_mut::<FocusedWidget>().0 = Some(editor_entity);

    // ── Enter キーを just_pressed に設定 ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Enter);
    }
    app.update();

    // ── CosmicTextChanged が発火したか確認 ──
    let changed_events: Vec<(Entity, String)> = app
        .world_mut()
        .resource_mut::<Messages<CosmicTextChanged>>()
        .drain()
        .map(|CosmicTextChanged(pair)| pair)
        .collect();

    assert_eq!(
        changed_events.len(),
        1,
        "Enter 入力で CosmicTextChanged が 1 件発火するはず (got {})",
        changed_events.len()
    );

    let new_text = &changed_events[0].1;
    // Enter で改行が入り 2 行になるはず。
    let lines: Vec<&str> = new_text.lines().collect();
    assert!(
        lines.len() >= 2,
        "Enter 後に少なくとも 2 行になるはず (lines={:?})",
        lines
    );

    // 新しい行 (lines[1]) が前行 (lines[0]) のインデント幅（4 スペース）で始まるはず。
    let first_indent = lines[0].chars().take_while(|c| *c == ' ').count();
    let second_indent = lines[1].chars().take_while(|c| *c == ' ').count();
    assert_eq!(
        second_indent,
        first_indent,
        "Enter 後の新行は前行と同じインデント幅を持つはず \
         (first_indent={first_indent}, second_indent={second_indent}, text={:?})",
        new_text
    );

    // Enter キーは reset されているはず (cosmic の二重改行防止)。
    {
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(
            !keys.just_pressed(KeyCode::Enter),
            "enter_autoindent_system が Enter キーを reset するはず"
        );
    }
}
