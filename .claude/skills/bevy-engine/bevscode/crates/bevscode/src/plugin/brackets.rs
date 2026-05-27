#![allow(dead_code)]

use crate::settings::*;
use crate::text_view::{DisplayLayout, TextBuffer};
use crate::types::*;
use bevy::prelude::*;
use bevy_instanced_text::{CornerRadii, MonoCellWidth, RectOverlay, RowVertical};
use bevy_instanced_text_editor::RopeBuffer;

type BracketMatchQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static CursorState,
        &'static TextBuffer<RopeBuffer>,
        &'static mut BracketMatchState,
        &'static BracketConfig,
        &'static AutoEdit,
    ),
    (
        With<CodeEditor>,
        Or<(Changed<CursorState>, Changed<TextBuffer<RopeBuffer>>)>,
    ),
>;

type BracketHighlightQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static TextBuffer<RopeBuffer>,
        &'static BracketMatchState,
        &'static FoldState,
        &'static MonoCellWidth,
        Option<&'static DisplayLayout>,
        &'static EditorTheme,
        &'static BracketConfig,
        &'static mut BracketMatchRects,
    ),
    With<CodeEditor>,
>;

pub struct BracketPlugin;

impl Plugin for BracketPlugin {
    fn build(&self, _app: &mut App) {}
}

pub(crate) fn find_matching_bracket(
    rope: &ropey::Rope,
    pos: usize,
    bracket_pairs: &[(char, char)],
) -> Option<BracketMatch> {
    if pos >= rope.len_chars() {
        return None;
    }

    let char_at_cursor = rope.char(pos);

    for &(open, close) in bracket_pairs {
        if char_at_cursor == open {
            if let Some(match_pos) = find_closing_bracket(rope, pos, open, close) {
                return Some(BracketMatch {
                    cursor_bracket_pos: pos,
                    matching_bracket_pos: match_pos,
                });
            }
        } else if char_at_cursor == close {
            if let Some(match_pos) = find_opening_bracket(rope, pos, open, close) {
                return Some(BracketMatch {
                    cursor_bracket_pos: pos,
                    matching_bracket_pos: match_pos,
                });
            }
        }
    }

    if pos > 0 {
        let char_before = rope.char(pos - 1);
        for &(open, close) in bracket_pairs {
            if char_before == open {
                if let Some(match_pos) = find_closing_bracket(rope, pos - 1, open, close) {
                    return Some(BracketMatch {
                        cursor_bracket_pos: pos - 1,
                        matching_bracket_pos: match_pos,
                    });
                }
            } else if char_before == close {
                if let Some(match_pos) = find_opening_bracket(rope, pos - 1, open, close) {
                    return Some(BracketMatch {
                        cursor_bracket_pos: pos - 1,
                        matching_bracket_pos: match_pos,
                    });
                }
            }
        }
    }

    None
}

pub(crate) fn find_closing_bracket(
    rope: &ropey::Rope,
    start_pos: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 1;
    let mut pos = start_pos + 1;
    let len = rope.len_chars();

    while pos < len && depth > 0 {
        let c = rope.char(pos);
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(pos);
            }
        }
        pos += 1;
    }

    None
}

pub(crate) fn find_opening_bracket(
    rope: &ropey::Rope,
    start_pos: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 1;
    let mut pos = start_pos;

    while pos > 0 && depth > 0 {
        pos -= 1;
        let c = rope.char(pos);
        if c == close {
            depth += 1;
        } else if c == open {
            depth -= 1;
            if depth == 0 {
                return Some(pos);
            }
        }
    }

    None
}

pub(crate) fn update_bracket_match(mut editor_query: BracketMatchQuery) {
    for (cursor, buffer, mut bracket_state, brackets, auto_edit) in editor_query.iter_mut() {
        if matches!(brackets.match_brackets, MatchBrackets::Never) {
            bracket_state.current_match = None;
            continue;
        }

        let cursor_pos = cursor.cursor_pos.min(buffer.len_chars());
        bracket_state.current_match =
            find_matching_bracket(buffer.rope(), cursor_pos, &auto_edit.pairs);
    }
}

pub(crate) fn update_bracket_highlight(mut editor_query: BracketHighlightQuery) {
    const BORDER: f32 = 2.0;

    for (buffer, bracket_state, fold_state, mono, layout, theme, brackets, mut bracket_rects) in
        editor_query.iter_mut()
    {
        let Some(bracket_match) = &bracket_state.current_match else {
            if !bracket_rects.0.is_empty() {
                bracket_rects.0.clear();
            }
            continue;
        };

        let use_box_style = matches!(
            brackets.style,
            BracketHighlightStyle::Background | BracketHighlightStyle::Both
        );
        let char_width = mono.px;

        let mut new_rects: Vec<RectOverlay> = Vec::new();
        for &bracket_pos in &[
            bracket_match.cursor_bracket_pos,
            bracket_match.matching_bracket_pos,
        ] {
            if bracket_pos >= buffer.len_chars() {
                continue;
            }
            let line_idx = buffer.char_to_line(bracket_pos);
            if fold_state.is_line_hidden(line_idx) {
                continue;
            }

            let line_start = buffer.line_to_char(line_idx);
            let col_idx = bracket_pos - line_start;
            let line = buffer.line(line_idx);
            let col_clamped = col_idx.min(line.len_chars());
            let next_col = (col_idx + 1).min(line.len_chars());
            let start_byte = line.slice(..col_clamped).len_bytes();
            let end_byte = line.slice(..next_col).len_bytes();

            let (start_row, start_byte_in_row) = layout
                .and_then(|l| l.buffer_to_display(line_idx as u32, start_byte))
                .unwrap_or_else(|| {
                    (
                        fold_state.actual_to_display_line(line_idx) as u32,
                        start_byte,
                    )
                });
            let (end_row, end_byte_in_row) = layout
                .and_then(|l| l.buffer_to_display(line_idx as u32, end_byte))
                .unwrap_or((start_row, end_byte));

            let glyph_x = layout
                .and_then(|l| l.x_at_byte(start_row, start_byte_in_row))
                .unwrap_or(col_idx as f32 * char_width);
            // Use `x_after_source_range` for the right edge: when the
            // bracket is the last source byte before an inlay hint (e.g.
            // `(` with a parameter hint anchored right after it), the
            // naive `x_at_byte(end_byte_in_row)` jumps past the virtual
            // span and the highlight box engulfs the inlay. The dedicated
            // helper stops at the source glyph's trailing edge.
            let glyph_width = if start_row == end_row {
                layout
                    .and_then(|l| {
                        let s = l.x_at_byte(start_row, start_byte_in_row)?;
                        let e =
                            l.x_after_source_range(start_row, start_byte_in_row, end_byte_in_row)?;
                        Some((e - s).max(0.0))
                    })
                    .unwrap_or(char_width)
            } else {
                char_width
            };

            let x0 = glyph_x;
            let x1 = glyph_x + glyph_width;
            let color = theme.bracket_match;

            if use_box_style {
                new_rects.push(RectOverlay {
                    display_row: start_row,
                    x_range: x0..x1,
                    vertical: RowVertical::TopBand { thickness: BORDER },
                    color,
                    z: 0,
                    corners: CornerRadii::ZERO,
                });
                new_rects.push(RectOverlay {
                    display_row: start_row,
                    x_range: x0..x1,
                    vertical: RowVertical::BottomBand { thickness: BORDER },
                    color,
                    z: 0,
                    corners: CornerRadii::ZERO,
                });
                new_rects.push(RectOverlay {
                    display_row: start_row,
                    x_range: x0..x0 + BORDER,
                    vertical: RowVertical::Full,
                    color,
                    z: 0,
                    corners: CornerRadii::ZERO,
                });
                new_rects.push(RectOverlay {
                    display_row: start_row,
                    x_range: x1 - BORDER..x1,
                    vertical: RowVertical::Full,
                    color,
                    z: 0,
                    corners: CornerRadii::ZERO,
                });
            } else {
                new_rects.push(RectOverlay {
                    display_row: start_row,
                    x_range: x0..x1,
                    vertical: RowVertical::Full,
                    color,
                    z: 0,
                    corners: CornerRadii::ZERO,
                });
            }
        }

        if bracket_rects.0 != new_rects {
            bracket_rects.0 = new_rects;
        }
    }
}
