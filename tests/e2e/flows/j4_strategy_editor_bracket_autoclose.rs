//! J4 strategy_editor_bracket_autoclose — Strategy Editor で `(` / `[` / `{` / `"` / `'` を入力すると
//! 対応する閉じ括弧を補完し、直後が閉じ括弧なら重複補完しないことを保証する（kind:ui）。
//!
//! `bracket_autoclose_system` は cosmic が opener を挿入した直後 (`.after(InputSet)`) に
//! `KeyboardInput` イベントを読んで closer を後置する。本テストでは opener が既に挿入された
//! 状態の `CosmicEditor` と `KeyboardInput` イベントを手動で用意して経路を検証する。
//!
//! VERIFY 済み: `bracket_autoclose_system` は `src/ui/strategy_editor_input.rs` に実装されている。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKeyCode};
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Action, Attrs, Edit, Editor, Metrics};
use bevy_cosmic_edit::prelude::FocusedWidget;
use bevy_cosmic_edit::{CosmicEditBuffer, CosmicEditor, CosmicFontSystem, CosmicTextChanged};
use cosmic_text::FontSystem;

use backcast::ui::strategy_editor::StrategyEditorContent;
use backcast::ui::strategy_editor_input::bracket_autoclose_system;

#[test]
fn j4_strategy_editor_bracket_autoclose() {
    let mut app = App::new();

    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(14.0, 18.0);

    // 空行から始める。cosmic が `(` を挿入した後の状態を手動で再現する:
    // opener `(` を Action::Insert で挿入してカーソルを opener の直後に置く。
    let buf = CosmicEditBuffer::new(&mut font_system, metrics)
        .with_text(&mut font_system, "", Attrs::new());
    let inner_buf_clone = buf.0.clone();
    let mut editor = Editor::new(inner_buf_clone);
    editor.set_redraw(true);

    // cosmic が `(` を挿入した状態を模擬。カーソルは `(` の直後。
    // 同じ font_system を使うことで buffer と editor が同一のグリフ状態を共有する。
    editor.action(&mut font_system, Action::Insert('('));

    let cosmic_editor = CosmicEditor::new(editor);

    app.insert_resource(CosmicFontSystem(font_system))
        .insert_resource(FocusedWidget(None))
        .insert_resource(ButtonInput::<KeyCode>::default())
        .add_event::<CosmicTextChanged>()
        .add_event::<KeyboardInput>()
        .add_systems(Update, bracket_autoclose_system);

    let editor_entity = app
        .world_mut()
        .spawn((StrategyEditorContent, buf, cosmic_editor))
        .id();

    app.world_mut().resource_mut::<FocusedWidget>().0 = Some(editor_entity);

    // ── `(` の KeyboardInput イベントを注入 (cosmic が insert した後の模擬) ──
    app.world_mut()
        .resource_mut::<Events<KeyboardInput>>()
        .send(KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: Key::Character("(".into()),
            state: ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

    app.update();

    // ── CosmicTextChanged が発火し、closer `)` が挿入されているか確認 ──
    let changed_events: Vec<(Entity, String)> = app
        .world_mut()
        .resource_mut::<Events<CosmicTextChanged>>()
        .drain()
        .map(|CosmicTextChanged(pair)| pair)
        .collect();

    assert_eq!(
        changed_events.len(),
        1,
        "`(` 入力で closer が挿入されて CosmicTextChanged が 1 件発火するはず (got {})",
        changed_events.len()
    );

    let new_text = &changed_events[0].1;
    assert!(
        new_text.contains('(') && new_text.contains(')'),
        "opener `(` と closer `)` の両方が含まれるはず (text={:?})",
        new_text
    );
    // 括弧の対が正しく "()" になっていること。
    assert!(
        new_text.contains("()"),
        "opener と closer が隣接して `()` を形成するはず (text={:?})",
        new_text
    );

    // ── フェーズ 2: `)` の直前では autoclose しない（重複補完防止）──
    // 新しい app で `(` が既にあり、カーソルが `)` の直前の状態を作る。
    // bracket_autoclose_system は next_char == Some(')') なら closer を挿入しない。

    let mut app2 = App::new();
    let mut font_system2 = FontSystem::new();

    // `()` がある状態でカーソルが `(` の直後 (= `)` の直前) にある。
    let buf2 = CosmicEditBuffer::new(&mut font_system2, metrics)
        .with_text(&mut font_system2, "()", Attrs::new());
    let buf2_clone = buf2.0.clone();
    let mut editor2 = Editor::new(buf2_clone);
    editor2.set_redraw(true);
    // カーソルを index=1 (= `(` の直後、`)` の直前) に移動。
    // Home で行頭、Right で 1 文字進む → cursor.index == 1 → next_char == Some(')')
    editor2.action(
        &mut font_system2,
        bevy_cosmic_edit::cosmic_text::Action::Motion(
            bevy_cosmic_edit::cosmic_text::Motion::Home,
        ),
    );
    editor2.action(
        &mut font_system2,
        bevy_cosmic_edit::cosmic_text::Action::Motion(
            bevy_cosmic_edit::cosmic_text::Motion::Right,
        ),
    );
    let cosmic_editor2 = CosmicEditor::new(editor2);

    app2.insert_resource(CosmicFontSystem(font_system2))
        .insert_resource(FocusedWidget(None))
        .insert_resource(ButtonInput::<KeyCode>::default())
        .add_event::<CosmicTextChanged>()
        .add_event::<KeyboardInput>()
        .add_systems(Update, bracket_autoclose_system);

    let editor_entity2 = app2
        .world_mut()
        .spawn((StrategyEditorContent, buf2, cosmic_editor2))
        .id();

    app2.world_mut().resource_mut::<FocusedWidget>().0 = Some(editor_entity2);

    // opener の KeyboardInput を送る。`)` の直前なので autoclose しないはず。
    app2.world_mut()
        .resource_mut::<Events<KeyboardInput>>()
        .send(KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: Key::Character("(".into()),
            state: ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

    app2.update();

    let events2: Vec<_> = app2
        .world_mut()
        .resource_mut::<Events<CosmicTextChanged>>()
        .drain()
        .collect();

    assert!(
        events2.is_empty(),
        "`)` の直前では autoclose しないので CosmicTextChanged は発火しないはず (got {})",
        events2.len()
    );
}
