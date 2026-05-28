//! Multi-cursor handlers — AddCursor{AtNextOccurrence,Above,Below},
//! ClearSecondaryCursors.

use crate::input::action_events::*;
use crate::input::picking_backend::add_cursor_at_next_occurrence;
use crate::types::*;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;

type EditorView<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut SelectionState,
        &'static mut CursorState,
        &'static crate::text_view::TextBuffer<RopeBuffer>,
    ),
    With<CodeEditor>,
>;

type EditorViewWithSelection<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut SelectionState,
        &'static mut CursorState,
        &'static crate::text_view::TextBuffer<RopeBuffer>,
        &'static crate::settings::SelectionConfig,
    ),
    With<CodeEditor>,
>;

pub fn handle_add_cursor_at_next_occurrence(
    mut events: MessageReader<AddCursorAtNextOccurrenceRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorViewWithSelection,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, selection_cfg)) = q.get_mut(entity) else {
        return;
    };
    if selection_cfg.limit > 0 && sel.selections.iter().count() >= selection_cfg.limit as usize {
        return;
    }
    let _ = add_cursor_at_next_occurrence(
        &mut sel,
        &mut cursor,
        buffer,
        &selection_cfg.word_separators,
    );
}

pub fn handle_add_cursor_above(
    mut events: MessageReader<AddCursorAboveRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer)) = q.get_mut(entity) else {
        return;
    };
    add_cursor_above(&mut sel, &mut cursor, buffer);
}

pub fn handle_add_cursor_below(
    mut events: MessageReader<AddCursorBelowRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer)) = q.get_mut(entity) else {
        return;
    };
    add_cursor_below(&mut sel, &mut cursor, buffer);
}

pub fn handle_clear_secondary_cursors(
    mut events: MessageReader<ClearSecondaryCursorsRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut cursor, _buffer)) = q.get_mut(entity) else {
        return;
    };
    if sel.has_multiple_cursors() {
        sel.clear_secondary_cursors(&mut cursor);
    }
}

/// Add a cursor on the line above the primary cursor.
fn add_cursor_above(
    sel: &mut SelectionState,
    cursor: &mut CursorState,
    buffer: &crate::text_view::TextBuffer<RopeBuffer>,
) {
    let primary_pos = sel.selections.primary().head_offset();
    let line_idx = buffer.char_to_line(primary_pos);

    if line_idx == 0 {
        return;
    }

    let line_start = buffer.line_to_char(line_idx);
    let col_offset = primary_pos - line_start;

    let prev_line_start = buffer.line_to_char(line_idx - 1);
    let prev_line_len = buffer.line(line_idx - 1).len_chars().saturating_sub(1);
    let new_pos = prev_line_start + col_offset.min(prev_line_len);

    sel.add_cursor_at(&**buffer, new_pos);
    sel.refresh_primary_cursor(cursor);
}

/// Add a cursor on the line below the primary cursor.
fn add_cursor_below(
    sel: &mut SelectionState,
    cursor: &mut CursorState,
    buffer: &crate::text_view::TextBuffer<RopeBuffer>,
) {
    let primary_pos = sel.selections.primary().head_offset();
    let line_idx = buffer.char_to_line(primary_pos);

    if line_idx + 1 >= buffer.len_lines() {
        return;
    }

    let line_start = buffer.line_to_char(line_idx);
    let col_offset = primary_pos - line_start;

    let next_line_start = buffer.line_to_char(line_idx + 1);
    let next_line_len = buffer.line(line_idx + 1).len_chars().saturating_sub(1);
    let new_pos = next_line_start + col_offset.min(next_line_len);

    sel.add_cursor_at(&**buffer, new_pos);
    sel.refresh_primary_cursor(cursor);
}
