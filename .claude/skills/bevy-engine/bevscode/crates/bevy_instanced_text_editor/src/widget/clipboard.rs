//! Clipboard handler systems: Copy, Cut, Paste.

use crate::history::EditKind;
use crate::text::RopeBuffer;
use crate::text_edit::*;
use crate::text_state::{EditHistoryState, TextEditor};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text::TextBuffer;
use bevy_instanced_text_interaction::{ClipboardResource, CursorState, SelectionState};

pub fn handle_copy(
    mut events: MessageReader<CopyRequested>,
    input_focus: Res<InputFocus>,
    clipboard: Res<ClipboardResource>,
    q: Query<(&SelectionState, &TextBuffer<RopeBuffer>), With<TextEditor>>,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((sel, buffer)) = q.get(entity) else {
        return;
    };
    if let Some((start, end)) = sel.primary_range() {
        let start = start.min(buffer.len_chars());
        let end = end.min(buffer.len_chars());
        let text = buffer.slice(start..end).to_string();
        clipboard.set_text(&text);
    }
}

pub fn handle_cut(
    mut events: MessageReader<CutRequested>,
    input_focus: Res<InputFocus>,
    clipboard: Res<ClipboardResource>,
    mut q: Query<
        (
            &mut SelectionState,
            &mut EditHistoryState,
            &mut CursorState,
            &mut TextBuffer<RopeBuffer>,
        ),
        With<TextEditor>,
    >,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut hist, mut cursor, mut buffer)) = q.get_mut(entity) else {
        return;
    };
    let Some((start, end)) = sel.primary_range() else {
        return;
    };
    let selected_text = buffer
        .slice(start.min(buffer.len_chars())..end.min(buffer.len_chars()))
        .to_string();

    clipboard.set_text(&selected_text);

    let outcome = hist.replace_range(&mut buffer, start, end, "", EditKind::Other, true);
    cursor.cursor_pos = outcome.new_cursor_pos;
    sel.apply_primary_cursor(&cursor);
}

pub fn handle_paste(
    mut events: MessageReader<PasteRequested>,
    input_focus: Res<InputFocus>,
    clipboard: Res<ClipboardResource>,
    mut q: Query<
        (
            &mut SelectionState,
            &mut EditHistoryState,
            &mut CursorState,
            &mut TextBuffer<RopeBuffer>,
        ),
        With<TextEditor>,
    >,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((mut sel, mut hist, mut cursor, mut buffer)) = q.get_mut(entity) else {
        return;
    };
    let Some(text) = clipboard.get_text() else {
        return;
    };

    let (start, end) = sel
        .primary_range()
        .unwrap_or((cursor.cursor_pos, cursor.cursor_pos));
    let outcome = hist.replace_range(&mut buffer, start, end, &text, EditKind::Paste, true);
    cursor.cursor_pos = outcome.new_cursor_pos;
    sel.apply_primary_cursor(&cursor);
}
