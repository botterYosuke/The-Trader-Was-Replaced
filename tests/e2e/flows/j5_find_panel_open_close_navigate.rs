//! J5 find_panel_open_close_navigate — Ctrl+F で Find / Replace パネルが開き、
//! クエリを設定するとマッチが計算され、FindActionRequested(Next/Prev) でナビゲーションでき、
//! Escape で閉じることを保証する（kind:ui）。
//!
//! Slice 5 (#50): cosmic `FocusedWidget` / `StrategyEditorContent` を撤去し、
//! Bevy native `InputFocus` + bevscode peer `StrategyEditorNode` 経路で書き直し。
//!
//! - `find_keyboard_system` が Ctrl+F で `FindReplaceState.is_open=true` にセット。
//! - `compute_find_match_spans_system` がクエリとフラグメントからマッチを計算。
//! - `find_navigate_system` が Next/Prev アクションで `state.current` を更新。
//! - `find_keyboard_system` が Escape で `is_open=false` にセット。
//! マッチ計算は純粋で headless 友好的。`manage_find_panel_lifecycle_system` （Bevy UI Node spawn）は
//! 本テストには含めない（panel UI の構造テストは別 flow）。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::Window;

use backcast::ui::components::{StrategyEditorId, StrategyFragment, WindowRoot};
use backcast::ui::strategy_editor::StrategyEditorNode;
use backcast::ui::strategy_editor_find::{
    FindActionRequested, FindButtonKind, FindMatchSpans, FindReplaceState,
    compute_find_match_spans_system, find_field_input_system, find_keyboard_system,
    find_navigate_system,
};

#[test]
fn j5_find_panel_open_close_navigate() {
    let mut app = App::new();

    // ── 最小リソース ──
    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<()>::default())
        .insert_resource(FindReplaceState::default())
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_message::<FindActionRequested>()
        .add_systems(
            Update,
            (
                find_keyboard_system,
                compute_find_match_spans_system,
                find_navigate_system,
            )
                .chain(),
        );

    let region_key = "region_001".to_string();
    let source = "foo bar foo baz foo";

    // WindowRoot + StrategyFragment（マッチ計算の対象ソース）。
    let root = app
        .world_mut()
        .spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            StrategyFragment {
                source: source.to_string(),
                dirty: false,
            },
        ))
        .id();

    // bevscode peer (StrategyEditorNode) entity — マッチスパンの書き込み先 + InputFocus の対象。
    let editor_entity = app
        .world_mut()
        .spawn((
            StrategyEditorNode {
                root,
                region_key: region_key.clone(),
            },
            FindMatchSpans::default(),
        ))
        .id();

    // InputFocus を editor entity に向ける（find_keyboard_system が target_editor に採用する）。
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .0 = Some(editor_entity);

    // ── Phase A: Ctrl+F で Find パネルを開く ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert!(state.is_open, "Ctrl+F で is_open=true になるはず");
        assert_eq!(
            state.target_editor,
            Some(editor_entity),
            "focus 中の editor entity が target_editor になるはず"
        );
    }

    // ── Phase B: クエリを注入してマッチを計算する ──
    // compute_find_match_spans_system は state.query の変化を Local で検知して再計算する。
    {
        let mut state = app.world_mut().resource_mut::<FindReplaceState>();
        state.query = "foo".to_string();
    }
    app.update(); // compute が走りマッチを埋める。

    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(
            state.matches.len(),
            3,
            "\"foo bar foo baz foo\" に \"foo\" が 3 件マッチするはず (got {})",
            state.matches.len()
        );
        assert_eq!(state.current, 0, "クエリ変更後は先頭マッチ (current=0) になるはず");
    }

    // FindMatchSpans にも書き込まれているはず。
    {
        let spans = app.world().get::<FindMatchSpans>(editor_entity).unwrap();
        assert_eq!(
            spans.matches.len(),
            3,
            "FindMatchSpans にも 3 件書き込まれるはず"
        );
    }

    // ── Phase C: Next で次のマッチへ移動 ──
    app.world_mut()
        .write_message(FindActionRequested(FindButtonKind::Next));
    app.update();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.current, 1, "Next 後 current=1 になるはず");
    }

    // Next をもう 1 回。
    app.world_mut()
        .write_message(FindActionRequested(FindButtonKind::Next));
    app.update();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.current, 2, "Next×2 後 current=2 になるはず");
    }

    // Next で末尾からラップアラウンドする。
    app.world_mut()
        .write_message(FindActionRequested(FindButtonKind::Next));
    app.update();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.current, 0, "末尾から Next で current=0 (ラップ) になるはず");
    }

    // ── Phase D: Prev で前のマッチへ移動 ──
    app.world_mut()
        .write_message(FindActionRequested(FindButtonKind::Prev));
    app.update();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(
            state.current,
            2,
            "先頭から Prev でラップして current=2 になるはず"
        );
    }

    // ── Phase E: Escape で閉じる ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Escape);
    }
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert!(!state.is_open, "Escape で is_open=false になるはず");
    }

    // ── Phase F: 閉じた後のフレームでマッチがクリアされる ──
    app.update(); // compute_find_match_spans_system が is_open=false を検知してクリア。

    {
        let state = app.world().resource::<FindReplaceState>();
        assert!(
            state.matches.is_empty(),
            "is_open=false 後にマッチがクリアされるはず (got {})",
            state.matches.len()
        );
    }
}

/// J5b — N7 (#50 followup): exercise find_field_input_system so Iter 1 N1/N3/N4 fixes
/// have regression coverage (Ctrl+F clears InputFocus, Ctrl+A doesn't pollute query,
/// Esc inside field restores focus to target_editor).
#[test]
fn j5b_find_field_input_handles_focus_typing_and_modifier_guard() {
    let mut app = App::new();

    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<()>::default())
        .insert_resource(FindReplaceState::default())
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_message::<FindActionRequested>()
        .add_message::<KeyboardInput>()
        .add_systems(
            Update,
            (
                find_keyboard_system,
                find_field_input_system,
            )
                .chain(),
        );

    let region_key = "region_001".to_string();
    let root = app
        .world_mut()
        .spawn((
            WindowRoot,
            StrategyEditorId { region_key: region_key.clone() },
            StrategyFragment { source: "abc abc xyz".to_string(), dirty: false },
        ))
        .id();
    let editor_entity = app
        .world_mut()
        .spawn((
            StrategyEditorNode { root, region_key: region_key.clone() },
            FindMatchSpans::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .0 = Some(editor_entity);

    // Phase A — Ctrl+F clears InputFocus (N1).
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();

    {
        let state = app.world().resource::<FindReplaceState>();
        assert!(state.is_open, "Ctrl+F should open the panel");
        assert_eq!(state.target_editor, Some(editor_entity));
        let focus = app.world().resource::<bevy::input_focus::InputFocus>();
        assert_eq!(focus.0, None, "N1: InputFocus must be None after Ctrl+F");
    }

    // Phase B — type "abc" into Find query.
    let dummy_window = app.world_mut().spawn(Window::default()).id();
    for ch in ["a", "b", "c"] {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character(ch.into()),
            state: ButtonState::Pressed,
            repeat: false,
            window: dummy_window,
            text: Some(ch.into()),
        });
    }
    app.update();
    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.query, "abc", "got query={:?}", state.query);
    }

    // Phase C — N4: Ctrl+A must NOT mutate the query.
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
    }
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()),
        state: ButtonState::Pressed,
        repeat: false,
        window: dummy_window,
        text: Some("a".into()),
    });
    app.update();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    {
        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.query, "abc", "N4: Ctrl+A polluted query: {:?}", state.query);
    }

    // Phase D — N3: Esc inside field restores InputFocus to target_editor.
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        repeat: false,
        window: dummy_window,
        text: None,
    });
    app.update();
    {
        let state = app.world().resource::<FindReplaceState>();
        assert!(!state.is_open, "Esc should close panel");
        let focus = app.world().resource::<bevy::input_focus::InputFocus>();
        assert_eq!(focus.0, Some(editor_entity), "N3: focus must restore");
    }
}
