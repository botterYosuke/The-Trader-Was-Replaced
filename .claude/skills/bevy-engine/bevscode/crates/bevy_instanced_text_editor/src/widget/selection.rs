//! Selection extension handler systems.

use crate::cursor_movement::{
    move_cursor, move_cursor_down_display, move_cursor_line_end_display,
    move_cursor_line_start_display, move_cursor_up_display, move_cursor_word_left,
    move_cursor_word_right,
};
use crate::text::RopeBuffer;
use crate::text_edit::*;
use crate::text_state::TextEditor;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text::{DisplayLayout, TextBuffer};
use bevy_instanced_text_interaction::{CursorState, SelectionState};

type EditorView<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut SelectionState,
        &'static mut CursorState,
        &'static TextBuffer<RopeBuffer>,
        Option<&'static DisplayLayout>,
    ),
    With<TextEditor>,
>;

pub fn handle_select_left(
    mut events: MessageReader<SelectLeftRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor(&mut cursor, buffer.rope(), -1);
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_right(
    mut events: MessageReader<SelectRightRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor(&mut cursor, buffer.rope(), 1);
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_up(
    mut events: MessageReader<SelectUpRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor_up_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_down(
    mut events: MessageReader<SelectDownRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor_down_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_word_left(
    mut events: MessageReader<SelectWordLeftRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor_word_left(&mut cursor, buffer.rope());
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_word_right(
    mut events: MessageReader<SelectWordRightRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor_word_right(&mut cursor, buffer.rope());
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_line_start(
    mut events: MessageReader<SelectLineStartRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor_line_start_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_line_end(
    mut events: MessageReader<SelectLineEndRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    let anchor = cursor.cursor_pos;
    move_cursor_line_end_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_with_anchor(&cursor, anchor);
}

pub fn handle_select_all(
    mut events: MessageReader<SelectAllRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, _cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    // Leave cursor in place so auto_scroll_to_cursor doesn't jump the viewport.
    let end = buffer.len_chars();
    sel.selections.set_selection(end, 0);
}

/// Drops secondary cursors first; otherwise collapses to a single caret.
pub fn handle_clear_selection(
    mut events: MessageReader<ClearSelectionRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, _buffer, _layout)) = q.get_mut(entity) else {
        return;
    };

    if sel.has_multiple_cursors() {
        sel.clear_secondary_cursors(&mut cursor);
        return;
    }

    sel.apply_primary_cursor(&cursor);
}
