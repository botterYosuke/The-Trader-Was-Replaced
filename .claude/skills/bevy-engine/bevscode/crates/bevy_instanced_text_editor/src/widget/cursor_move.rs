//! Cursor movement handler systems. Selection is cleared; use the selection
//! handlers for shift-extended moves.

use crate::cursor_movement::{
    move_cursor, move_cursor_down_display, move_cursor_line_end_display,
    move_cursor_line_start_display, move_cursor_lines_display, move_cursor_up_display,
    move_cursor_word_left, move_cursor_word_right,
};
use crate::text::RopeBuffer;
use crate::text_edit::*;
use crate::text_state::TextEditor;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy_instanced_text::{DisplayLayout, MonoCellWidth, TextBuffer};
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

fn focused(input_focus: &InputFocus) -> Option<Entity> {
    input_focus.get()
}

pub fn handle_move_cursor_left(
    mut events: MessageReader<MoveCursorLeftRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor(&mut cursor, buffer.rope(), -1);
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_right(
    mut events: MessageReader<MoveCursorRightRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor(&mut cursor, buffer.rope(), 1);
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_up(
    mut events: MessageReader<MoveCursorUpRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor_up_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_down(
    mut events: MessageReader<MoveCursorDownRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor_down_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_word_left(
    mut events: MessageReader<MoveCursorWordLeftRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor_word_left(&mut cursor, buffer.rope());
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_word_right(
    mut events: MessageReader<MoveCursorWordRightRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor_word_right(&mut cursor, buffer.rope());
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_line_start(
    mut events: MessageReader<MoveCursorLineStartRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor_line_start_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_line_end(
    mut events: MessageReader<MoveCursorLineEndRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, layout)) = q.get_mut(entity) else {
        return;
    };
    move_cursor_line_end_display(&mut cursor, buffer.rope(), layout);
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_document_start(
    mut events: MessageReader<MoveCursorDocumentStartRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, _buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    cursor.cursor_pos = 0;
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_document_end(
    mut events: MessageReader<MoveCursorDocumentEndRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, _layout)) = q.get_mut(entity) else {
        return;
    };
    cursor.cursor_pos = buffer.len_chars();
    sel.apply_primary_cursor(&cursor);
}

type PagingView<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut SelectionState,
        &'static mut CursorState,
        &'static TextBuffer<RopeBuffer>,
        &'static ComputedNode,
        &'static TextFont,
        &'static bevy::text::LineHeight,
        &'static MonoCellWidth,
        Option<&'static DisplayLayout>,
    ),
    With<TextEditor>,
>;

/// Visible lines minus one overlap line for context.
fn page_lines(computed: &ComputedNode, line_height: f32) -> isize {
    if line_height <= 0.0 {
        return 1;
    }
    let height = computed.size().y * computed.inverse_scale_factor();
    let visible = (height / line_height).floor() as isize;
    (visible - 1).max(1)
}

pub fn handle_move_cursor_page_up(
    mut events: MessageReader<MoveCursorPageUpRequested>,
    input_focus: Res<InputFocus>,
    mut q: PagingView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, computed, font, lh, _mono, layout)) = q.get_mut(entity)
    else {
        return;
    };
    move_cursor_lines_display(
        &mut cursor,
        buffer.rope(),
        layout,
        -page_lines(
            computed,
            bevy_instanced_text::resolve_line_height(*lh, font.font_size),
        ),
    );
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_move_cursor_page_down(
    mut events: MessageReader<MoveCursorPageDownRequested>,
    input_focus: Res<InputFocus>,
    mut q: PagingView,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = focused(&input_focus) else {
        return;
    };
    let Ok((mut sel, mut cursor, buffer, computed, font, lh, _mono, layout)) = q.get_mut(entity)
    else {
        return;
    };
    move_cursor_lines_display(
        &mut cursor,
        buffer.rope(),
        layout,
        page_lines(
            computed,
            bevy_instanced_text::resolve_line_height(*lh, font.font_size),
        ),
    );
    sel.apply_primary_cursor(&cursor);
}
