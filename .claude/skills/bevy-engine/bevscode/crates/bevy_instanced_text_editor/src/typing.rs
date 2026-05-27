//! Typed-character observer for the focused [`crate::TextEditor`].
//!
//! Modifier keys (Ctrl, Cmd, Alt) skip insertion so they reach the host's
//! editing-event dispatcher.

use crate::text::RopeBuffer;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy_instanced_text::TextBuffer;

use crate::text_state::{EditHistoryState, TextEditor};
use crate::widget::text_input::insert_char;
use bevy_instanced_text_interaction::{CursorState, SelectionState};

fn modifier_held(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight)
        || keyboard.pressed(KeyCode::SuperLeft)
        || keyboard.pressed(KeyCode::SuperRight)
        || keyboard.pressed(KeyCode::AltLeft)
        || keyboard.pressed(KeyCode::AltRight)
}

pub fn on_focused_keyboard_typing(
    trigger: On<FocusedInput<KeyboardInput>>,
    mut q: Query<
        (
            &mut SelectionState,
            &mut EditHistoryState,
            &mut CursorState,
            &mut TextBuffer<RopeBuffer>,
        ),
        With<TextEditor>,
    >,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let entity = trigger.event().focused_entity;
    let Ok((mut sel, mut hist, mut cursor, mut buffer)) = q.get_mut(entity) else {
        return;
    };

    let event = &trigger.event().input;
    if !event.state.is_pressed() {
        return;
    }

    if modifier_held(&keyboard) {
        return;
    }

    match &event.logical_key {
        Key::Character(text) => {
            for c in text.chars() {
                if c.is_control() {
                    continue;
                }
                insert_char(&mut sel, &mut hist, &mut cursor, &mut buffer, c);
            }
        }
        Key::Space => {
            insert_char(&mut sel, &mut hist, &mut cursor, &mut buffer, ' ');
        }
        _ => {}
    }
}
