//! Buffer edit handlers: insert, delete, undo, redo.

use crate::history::EditKind;
use crate::text::RopeBuffer;
use crate::text_edit::*;
use crate::text_state::{EditHistoryState, IndentConfig, TextEditor};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text::{ContentMetrics, TextBuffer};
use bevy_instanced_text_interaction::{CursorState, SelectionState};

type EditorBufQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut SelectionState,
        &'static mut EditHistoryState,
        &'static mut CursorState,
        &'static mut TextBuffer<RopeBuffer>,
    ),
    With<TextEditor>,
>;

type EditorSetTextQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut SelectionState,
        &'static mut EditHistoryState,
        &'static mut CursorState,
        &'static mut TextBuffer<RopeBuffer>,
        &'static mut ContentMetrics,
    ),
    With<TextEditor>,
>;

pub fn insert_char(
    sel: &mut SelectionState,
    hist: &mut EditHistoryState,
    cursor: &mut CursorState,
    buffer: &mut TextBuffer<RopeBuffer>,
    c: char,
) {
    if sel.selections.primary().has_selection() {
        delete_selection(sel, hist, cursor, buffer);
    }
    hist.insert_char(sel, cursor, buffer, c);
}

pub fn delete_selection(
    sel: &mut SelectionState,
    hist: &mut EditHistoryState,
    cursor: &mut CursorState,
    buffer: &mut TextBuffer<RopeBuffer>,
) {
    let Some((start, end)) = sel.primary_range() else {
        return;
    };
    let outcome = hist.replace_range(buffer, start, end, "", EditKind::Other, true);
    cursor.cursor_pos = outcome.new_cursor_pos;
    sel.apply_primary_cursor(cursor);
}

pub fn handle_insert_newline(
    mut events: MessageReader<InsertNewlineRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
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
    insert_char(&mut sel, &mut hist, &mut cursor, &mut buffer, '\n');
}

pub fn handle_insert_tab(
    mut events: MessageReader<InsertTabRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
    indent_q: Query<&IndentConfig, With<TextEditor>>,
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
    let indent = indent_q.get(entity).copied().unwrap_or_default();
    if indent.use_spaces {
        let count = if indent.use_tab_stops && indent.tab_width > 0 {
            let pos = cursor.cursor_pos;
            let line = buffer.char_to_line(pos);
            let col = pos - buffer.line_to_char(line);
            indent.tab_width - (col % indent.tab_width)
        } else {
            indent.tab_width
        };
        for _ in 0..count {
            insert_char(&mut sel, &mut hist, &mut cursor, &mut buffer, ' ');
        }
    } else {
        insert_char(&mut sel, &mut hist, &mut cursor, &mut buffer, '\t');
    }
}

pub fn handle_delete_backward(
    mut events: MessageReader<DeleteBackwardRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
    indent_q: Query<&IndentConfig, With<TextEditor>>,
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
    let _span = bevy::prelude::info_span!("delete_backward").entered();
    if sel.selections.primary().has_selection() {
        delete_selection(&mut sel, &mut hist, &mut cursor, &mut buffer);
        return;
    }

    let indent = indent_q.get(entity).copied().unwrap_or_default();
    let pos = cursor.cursor_pos;
    if pos == 0 {
        return;
    }

    let rope = buffer.rope();
    let line = rope.char_to_line(pos);
    let line_start = rope.line_to_char(line);
    let col = pos - line_start;

    let sticky_target = if indent.sticky_tab_stops && indent.tab_width > 0 && col > 0 {
        let leading_spaces = rope
            .slice(line_start..line_start + col)
            .chars()
            .take_while(|c| *c == ' ')
            .count();
        if leading_spaces == col && col >= indent.tab_width {
            let stop = ((col - 1) / indent.tab_width) * indent.tab_width;
            Some(line_start + stop)
        } else {
            None
        }
    } else {
        None
    };

    let trim_target = if sticky_target.is_none() && indent.trim_whitespace_on_delete && col > 0 {
        let line_chars: Vec<char> = rope.slice(line_start..pos).chars().collect();
        let is_ws = |c: char| c == ' ' || c == '\t';
        if line_chars.last().is_some_and(|c| is_ws(*c)) {
            let mut run = line_chars.len();
            while run > 0 && is_ws(line_chars[run - 1]) {
                run -= 1;
            }
            if line_chars.len() - run > 1 {
                Some(line_start + run)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    if let Some(target_start) = sticky_target.or(trim_target) {
        let outcome = hist.replace_range(
            &mut buffer,
            target_start,
            pos,
            "",
            crate::history::EditKind::DeleteBackward,
            true,
        );
        cursor.cursor_pos = outcome.new_cursor_pos;
        sel.apply_primary_cursor(&cursor);
        return;
    }

    if crate::text_state::is_auto_pair_neighbor(buffer.rope(), pos) {
        let outcome = hist.replace_range(
            &mut buffer,
            pos.saturating_sub(1),
            pos + 1,
            "",
            crate::history::EditKind::DeleteBackward,
            true,
        );
        cursor.cursor_pos = outcome.new_cursor_pos;
        sel.apply_primary_cursor(&cursor);
    } else {
        hist.delete_backward(&mut sel, &mut cursor, &mut buffer);
    }
}

pub fn handle_delete_forward(
    mut events: MessageReader<DeleteForwardRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
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
    if sel.selections.primary().has_selection() {
        delete_selection(&mut sel, &mut hist, &mut cursor, &mut buffer);
    } else {
        hist.delete_forward(&mut sel, &mut cursor, &mut buffer);
    }
}

pub fn handle_delete_word_backward(
    mut events: MessageReader<DeleteWordBackwardRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
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
    if sel.selections.primary().has_selection() {
        delete_selection(&mut sel, &mut hist, &mut cursor, &mut buffer);
    } else {
        delete_word_backward(&mut sel, &mut hist, &mut cursor, &mut buffer);
    }
}

pub fn handle_delete_word_forward(
    mut events: MessageReader<DeleteWordForwardRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
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
    if sel.selections.primary().has_selection() {
        delete_selection(&mut sel, &mut hist, &mut cursor, &mut buffer);
    } else {
        delete_word_forward(&mut sel, &mut hist, &mut cursor, &mut buffer);
    }
}

pub fn handle_delete_line(mut events: MessageReader<DeleteLineRequested>) {
    events.read().for_each(|_| {});
}

pub fn handle_undo(
    mut events: MessageReader<UndoRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
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
    let _ = hist.undo(&mut sel, &mut cursor, &mut buffer);
}

pub fn handle_redo(
    mut events: MessageReader<RedoRequested>,
    input_focus: Res<InputFocus>,
    mut q: EditorBufQuery,
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
    let _ = hist.redo(&mut sel, &mut cursor, &mut buffer);
}

pub fn delete_word_backward(
    sel: &mut SelectionState,
    hist: &mut EditHistoryState,
    cursor: &mut CursorState,
    buffer: &mut TextBuffer<RopeBuffer>,
) {
    let word_start =
        crate::cursor_movement::find_word_boundary_left(buffer.rope(), cursor.cursor_pos);
    if word_start >= cursor.cursor_pos {
        return;
    }
    let outcome = hist.replace_range(
        buffer,
        word_start,
        cursor.cursor_pos,
        "",
        EditKind::DeleteBackward,
        true,
    );
    cursor.cursor_pos = outcome.new_cursor_pos;
    sel.apply_primary_cursor(cursor);
}

pub fn delete_word_forward(
    sel: &mut SelectionState,
    hist: &mut EditHistoryState,
    cursor: &mut CursorState,
    buffer: &mut TextBuffer<RopeBuffer>,
) {
    let word_end =
        crate::cursor_movement::find_word_boundary_right(buffer.rope(), cursor.cursor_pos);
    if word_end <= cursor.cursor_pos {
        return;
    }
    hist.replace_range(
        buffer,
        cursor.cursor_pos,
        word_end,
        "",
        EditKind::DeleteForward,
        true,
    );
    sel.apply_primary_cursor(cursor);
}

pub fn handle_replace_range(
    mut events: MessageReader<ReplaceRangeRequested>,
    mut q: EditorBufQuery,
) {
    for event in events.read() {
        let Ok((mut sel, mut hist, mut cursor, mut buffer)) = q.get_mut(event.entity) else {
            continue;
        };
        let outcome = hist.replace_range(
            &mut buffer,
            event.start,
            event.end,
            &event.text,
            event.kind,
            event.record_history,
        );
        if cursor.cursor_pos >= event.start {
            cursor.cursor_pos = outcome.new_cursor_pos;
            sel.apply_primary_cursor(&cursor);
        }
    }
}

pub fn handle_set_text(mut events: MessageReader<SetTextRequested>, mut q: EditorSetTextQuery) {
    for event in events.read() {
        let Ok((mut sel, mut hist, mut cursor, mut buffer, mut metrics)) = q.get_mut(event.entity)
        else {
            continue;
        };
        hist.set_text(
            &mut sel,
            &mut cursor,
            &mut buffer,
            &mut metrics,
            &event.text,
        );
    }
}
