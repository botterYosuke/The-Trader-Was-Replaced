//! Cursor movement and word-boundary helpers.
//!
//! `_display` variants walk display rows (soft-wrap aware) when given a
//! [`DisplayLayout`]; they fall back to rope-only movement when `None`.

use bevy_instanced_text::{DisplayLayout, ShapedLine};
use bevy_instanced_text_interaction::CursorState;
use ropey::Rope;

pub fn move_cursor_up(cursor: &mut CursorState, rope: &Rope) {
    if cursor.cursor_pos > 0 {
        let line_idx = rope.char_to_line(cursor.cursor_pos);
        if line_idx > 0 {
            let line_start = rope.line_to_char(line_idx);
            let col_offset = cursor.cursor_pos - line_start;
            let prev_line_start = rope.line_to_char(line_idx - 1);
            let prev_line_len = rope.line(line_idx - 1).len_chars();
            cursor.cursor_pos = prev_line_start + col_offset.min(prev_line_len.saturating_sub(1));
        }
    }
}

pub fn move_cursor_down(cursor: &mut CursorState, rope: &Rope) {
    let line_idx = rope.char_to_line(cursor.cursor_pos);
    if line_idx + 1 < rope.len_lines() {
        let line_start = rope.line_to_char(line_idx);
        let col_offset = cursor.cursor_pos - line_start;
        let next_line_start = rope.line_to_char(line_idx + 1);
        let next_line_len = rope.line(line_idx + 1).len_chars();
        cursor.cursor_pos = next_line_start + col_offset.min(next_line_len.saturating_sub(1));
    }
}

pub fn move_cursor_line_start(cursor: &mut CursorState, rope: &Rope) {
    let line_idx = rope.char_to_line(cursor.cursor_pos);
    cursor.cursor_pos = rope.line_to_char(line_idx);
}

pub fn move_cursor_line_end(cursor: &mut CursorState, rope: &Rope) {
    let line_idx = rope.char_to_line(cursor.cursor_pos);
    let line_start = rope.line_to_char(line_idx);
    let line_len = rope.line(line_idx).len_chars();
    cursor.cursor_pos = line_start + line_len.saturating_sub(1);
}

pub fn move_cursor(cursor: &mut CursorState, rope: &Rope, delta: isize) {
    if delta < 0 {
        let amount = (-delta) as usize;
        cursor.cursor_pos = cursor.cursor_pos.saturating_sub(amount);
    } else {
        let amount = delta as usize;
        cursor.cursor_pos = (cursor.cursor_pos + amount).min(rope.len_chars());
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

fn classify_char(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

/// Skips trailing whitespace, then characters of the same class.
pub fn find_word_boundary_left(rope: &Rope, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }

    let mut current = pos;

    while current > 0 {
        let c = rope.char(current - 1);
        if c.is_whitespace() && c != '\n' {
            current -= 1;
        } else {
            break;
        }
    }

    if current == 0 {
        return 0;
    }

    let class = classify_char(rope.char(current - 1));

    while current > 0 {
        let c = rope.char(current - 1);
        if c == '\n' {
            break;
        }
        if classify_char(c) == class {
            current -= 1;
        } else {
            break;
        }
    }

    current
}

pub fn find_word_boundary_right(rope: &Rope, pos: usize) -> usize {
    let len = rope.len_chars();
    if pos >= len {
        return len;
    }

    let mut current = pos;

    let c = rope.char(current);

    if c.is_whitespace() {
        while current < len {
            let c = rope.char(current);
            if c == '\n' {
                current += 1;
                return current.min(len);
            }
            if c.is_whitespace() {
                current += 1;
            } else {
                break;
            }
        }
        return current;
    }

    let class = classify_char(c);
    while current < len {
        let c = rope.char(current);
        if c == '\n' {
            break;
        }
        if classify_char(c) == class {
            current += 1;
        } else {
            break;
        }
    }

    while current < len {
        let c = rope.char(current);
        if c.is_whitespace() && c != '\n' {
            current += 1;
        } else {
            break;
        }
    }

    current
}

pub fn move_cursor_word_left(cursor: &mut CursorState, rope: &Rope) {
    cursor.cursor_pos = find_word_boundary_left(rope, cursor.cursor_pos);
}

pub fn move_cursor_word_right(cursor: &mut CursorState, rope: &Rope) {
    cursor.cursor_pos = find_word_boundary_right(rope, cursor.cursor_pos);
}

pub fn move_cursor_up_display(
    cursor: &mut CursorState,
    rope: &Rope,
    layout: Option<&DisplayLayout>,
) {
    let Some(layout) = layout else {
        move_cursor_up(cursor, rope);
        return;
    };
    move_cursor_display_rows(cursor, rope, layout, -1);
}

pub fn move_cursor_down_display(
    cursor: &mut CursorState,
    rope: &Rope,
    layout: Option<&DisplayLayout>,
) {
    let Some(layout) = layout else {
        move_cursor_down(cursor, rope);
        return;
    };
    move_cursor_display_rows(cursor, rope, layout, 1);
}

/// Home lands at the current display row's left edge, not the buffer line start.
pub fn move_cursor_line_start_display(
    cursor: &mut CursorState,
    rope: &Rope,
    layout: Option<&DisplayLayout>,
) {
    let Some(layout) = layout else {
        move_cursor_line_start(cursor, rope);
        return;
    };
    let Some((row, _, _)) = cursor_to_display(cursor.cursor_pos, rope, layout) else {
        move_cursor_line_start(cursor, rope);
        return;
    };
    let buffer_row = rope.char_to_line(cursor.cursor_pos);
    let line_start_char = rope.line_to_char(buffer_row);
    let line_start_byte = rope.line_to_byte(buffer_row);
    cursor.cursor_pos =
        display_row_byte_to_char(rope, layout, row, 0, buffer_row).unwrap_or_else(|| {
            line_start_char
                + rope.byte_to_char(line_start_byte + row_buffer_byte_offset(layout, row))
                - rope.byte_to_char(line_start_byte)
        });
}

/// End lands at the current display row's right edge, not the buffer line end.
pub fn move_cursor_line_end_display(
    cursor: &mut CursorState,
    rope: &Rope,
    layout: Option<&DisplayLayout>,
) {
    let Some(layout) = layout else {
        move_cursor_line_end(cursor, rope);
        return;
    };
    let Some((row, _, line)) = cursor_to_display(cursor.cursor_pos, rope, layout) else {
        move_cursor_line_end(cursor, rope);
        return;
    };
    let buffer_row = line.buffer_row as usize;
    let line_start_byte = rope.line_to_byte(buffer_row);
    let row_end_byte_in_line = line.buffer_byte_offset + line.text.len();
    let abs_byte = (line_start_byte + row_end_byte_in_line).min(rope.len_bytes());
    let mut pos = rope.byte_to_char(abs_byte);
    let line_chars = rope.line(buffer_row).len_chars();
    let line_last = rope.line_to_char(buffer_row) + line_chars.saturating_sub(1);
    pos = pos.min(line_last);
    cursor.cursor_pos = pos;
    let _ = row; // silence unused warning when display_row not needed downstream
}

pub fn move_cursor_lines_display(
    cursor: &mut CursorState,
    rope: &Rope,
    layout: Option<&DisplayLayout>,
    lines: isize,
) {
    let Some(layout) = layout else {
        move_cursor_lines(cursor, rope, lines);
        return;
    };
    move_cursor_display_rows(cursor, rope, layout, lines);
}

/// Projects the cursor's pixel-x onto target display row `cur + delta`.
fn move_cursor_display_rows(
    cursor: &mut CursorState,
    rope: &Rope,
    layout: &DisplayLayout,
    delta: isize,
) {
    let Some((cur_row, cur_byte_in_row, _)) = cursor_to_display(cursor.cursor_pos, rope, layout)
    else {
        return;
    };
    let cur_x = layout.x_at_byte(cur_row, cur_byte_in_row).unwrap_or(0.0);
    let target_row_i = cur_row as isize + delta;
    if target_row_i < 0 {
        cursor.cursor_pos = 0;
        return;
    }
    let target_row = target_row_i as u32;
    let target_line = match layout.lines.iter().find(|l| l.display_row == target_row) {
        Some(l) => l,
        None => return,
    };
    let target_byte_in_row = layout
        .byte_at_x(target_row, cur_x)
        .unwrap_or(0)
        .min(target_line.text.len());
    let target_buffer_row = target_line.buffer_row as usize;
    let buffer_line_start_byte = rope.line_to_byte(target_buffer_row);
    let abs_byte = (buffer_line_start_byte + target_line.buffer_byte_offset + target_byte_in_row)
        .min(rope.len_bytes());
    cursor.cursor_pos = rope.byte_to_char(abs_byte);
}

/// Returns `None` if the cursor's buffer row is off-viewport.
fn cursor_to_display<'a>(
    cursor_pos: usize,
    rope: &Rope,
    layout: &'a DisplayLayout,
) -> Option<(u32, usize, &'a ShapedLine)> {
    let buffer_row = rope.char_to_line(cursor_pos);
    let line_start_byte = rope.line_to_byte(buffer_row);
    let cursor_byte = rope.char_to_byte(cursor_pos);
    let byte_in_line = cursor_byte.saturating_sub(line_start_byte);
    let (row, byte_in_row) = layout.buffer_to_display(buffer_row as u32, byte_in_line)?;
    let line = layout.lines.iter().find(|l| l.display_row == row)?;
    Some((row, byte_in_row, line))
}

fn row_buffer_byte_offset(layout: &DisplayLayout, display_row: u32) -> usize {
    layout
        .lines
        .iter()
        .find(|l| l.display_row == display_row)
        .map(|l| l.buffer_byte_offset)
        .unwrap_or(0)
}

fn display_row_byte_to_char(
    rope: &Rope,
    layout: &DisplayLayout,
    display_row: u32,
    byte_in_row: usize,
    buffer_row_hint: usize,
) -> Option<usize> {
    let line = layout.lines.iter().find(|l| l.display_row == display_row)?;
    let buffer_row = line.buffer_row as usize;
    let line_start_byte = rope.line_to_byte(buffer_row);
    let abs_byte = (line_start_byte + line.buffer_byte_offset + byte_in_row).min(rope.len_bytes());
    let _ = buffer_row_hint;
    Some(rope.byte_to_char(abs_byte))
}

pub fn move_cursor_lines(cursor: &mut CursorState, rope: &Rope, lines: isize) {
    if lines == 0 {
        return;
    }
    let line_idx = rope.char_to_line(cursor.cursor_pos);
    let line_start = rope.line_to_char(line_idx);
    let col_offset = cursor.cursor_pos - line_start;
    let last_line = rope.len_lines().saturating_sub(1);
    let target = if lines < 0 {
        line_idx.saturating_sub((-lines) as usize)
    } else {
        (line_idx + lines as usize).min(last_line)
    };
    let target_start = rope.line_to_char(target);
    let target_len = rope.line(target).len_chars();
    cursor.cursor_pos = target_start + col_offset.min(target_len.saturating_sub(1));
}
