//! GotoLine dialog handler.

use crate::input::action_events::*;
use crate::types::*;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;

pub fn handle_goto_line(
    mut events: MessageReader<GotoLineRequested>,
    input_focus: Res<InputFocus>,
    mut q: Query<&mut GotoLineState, With<CodeEditor>>,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok(mut goto_line_state) = q.get_mut(entity) else {
        return;
    };
    goto_line_state.active = !goto_line_state.active;
    if goto_line_state.active {
        goto_line_state.input.clear();
    }
}
