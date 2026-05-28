//! Text search and cursor operations on the editor's rope buffer.

use crate::input::word_boundary::is_word_char;
use crate::text_view::TextBuffer;
use crate::types::*;
use bevy_instanced_text_editor::RopeBuffer;
use ropey::Rope;

pub fn move_cursor(cursor: &mut CursorState, rope: &Rope, delta: isize) {
    if delta < 0 {
        let amount = (-delta) as usize;
        cursor.cursor_pos = cursor.cursor_pos.saturating_sub(amount);
    } else {
        let amount = delta as usize;
        cursor.cursor_pos = (cursor.cursor_pos + amount).min(rope.len_chars());
    }
}

pub fn word_at_position(rope: &Rope, pos: usize, separators: &str) -> Option<(usize, usize)> {
    let pos = pos.min(rope.len_chars());
    if pos >= rope.len_chars() {
        return None;
    }
    if !is_word_char(rope.char(pos), separators) {
        return None;
    }
    let start = crate::input::word_boundary::find_word_start(rope, pos, separators);
    let end = crate::input::word_boundary::find_word_end(rope, pos, separators);
    if start < end {
        Some((start, end))
    } else {
        None
    }
}

pub fn find_next_occurrence(rope: &Rope, text: &str, after_pos: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }

    let text_chars: Vec<char> = text.chars().collect();
    let text_len = text_chars.len();
    let rope_len = rope.len_chars();

    let mut pos = after_pos;
    while pos + text_len <= rope_len {
        let mut matches = true;
        for (i, &tc) in text_chars.iter().enumerate() {
            if rope.char(pos + i) != tc {
                matches = false;
                break;
            }
        }
        if matches {
            return Some((pos, pos + text_len));
        }
        pos += 1;
    }

    pos = 0;
    while pos + text_len <= after_pos && pos + text_len <= rope_len {
        let mut matches = true;
        for (i, &tc) in text_chars.iter().enumerate() {
            if rope.char(pos + i) != tc {
                matches = false;
                break;
            }
        }
        if matches {
            return Some((pos, pos + text_len));
        }
        pos += 1;
    }

    None
}

pub fn add_cursor_at_next_occurrence(
    sel: &mut SelectionState,
    cursor: &mut CursorState,
    buffer: &TextBuffer<RopeBuffer>,
    separators: &str,
) -> bool {
    let primary = sel.selections.primary();
    let search_text = if primary.has_selection() {
        let (start, end) = primary.range();
        buffer.slice(start..end).to_string()
    } else if let Some((start, end)) =
        word_at_position(buffer.rope(), primary.head_offset(), separators)
    {
        // First Cmd+D on a bare cursor: select the word under the cursor.
        // Match the legacy behavior of placing the head at `end` (so the
        // caret sits at the end of the word) and the anchor at `start`.
        sel.selections.set_selection(end, start);
        sel.refresh_primary_cursor(cursor);
        return true;
    } else {
        return false;
    };

    if search_text.is_empty() {
        return false;
    }

    let search_from = sel.selections.iter().map(|s| s.end()).max().unwrap_or(0);

    if let Some((start, end)) = find_next_occurrence(buffer.rope(), &search_text, search_from) {
        let already_covered = sel.selections.iter().any(|s| {
            let (cs, ce) = s.range();
            start >= cs && end <= ce
        });

        if !already_covered {
            sel.add_cursor_with_range(&**buffer, end, start);
            sel.refresh_primary_cursor(cursor);
            return true;
        }
    }

    false
}
