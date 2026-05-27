//! UI elements: selection, indent guides

use crate::settings::*;
use crate::text_view::{DisplayLayout, RectOverlay, RowVertical, TextBuffer};
use crate::types::*;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::{visible_buffer_range, HiddenLines, MonoCellWidth, TextBounds};
use bevy_instanced_text_editor::RopeBuffer;

/// Iterate `(buffer_line, display_row)` pairs over the visible window,
/// skipping folded lines.
fn visible_display_rows(
    fold: &FoldState,
    total_lines: usize,
    line_height: f32,
    scroll_y: f32,
    viewport_height: f32,
) -> Vec<(usize, usize)> {
    let visible_start_row = (scroll_y / line_height).floor().max(0.0) as usize;
    let visible_lines = ((viewport_height / line_height).ceil() as usize) + 2;
    let visible_end_row = visible_start_row + visible_lines;
    let has_folding = !fold.regions.is_empty();

    let start_buffer_line = if has_folding {
        let mut display_row = 0;
        let mut buffer_line = 0;
        while buffer_line < total_lines && display_row < visible_start_row {
            if !fold.is_line_hidden(buffer_line) {
                display_row += 1;
            }
            buffer_line += 1;
        }
        buffer_line
    } else {
        visible_start_row.min(total_lines)
    };

    let mut current_display_row: usize = if has_folding {
        let mut display_row = 0;
        for bl in 0..start_buffer_line {
            if !fold.is_line_hidden(bl) {
                display_row += 1;
            }
        }
        display_row
    } else {
        start_buffer_line
    };

    let mut out = Vec::new();
    for buffer_line in start_buffer_line..total_lines {
        if fold.is_line_hidden(buffer_line) {
            continue;
        }
        if current_display_row > visible_end_row {
            break;
        }
        out.push((buffer_line, current_display_row));
        current_display_row += 1;
    }
    out
}

type AutoScrollQuery<'w, 's> = Query<
    'w,
    's,
    (
        EditorRenderView,
        ScrollTargetView,
    ),
    With<CodeEditor>,
>;

/// Push selection rectangles into `TextViewOverlays` for all cursors.
///
/// Selections render as paint-time overlay rects with `z = -1` (below text),
/// not as separate `Sprite` entities, so the engine's renderer paints them
/// in the same draw call as the glyphs.
///
/// Visible-window clipped: a selection covering the entire 150k-line buffer
/// only emits rects for the ~50 lines actually on screen. The selection
/// itself still spans the whole buffer (kept by `SelectionState`); we just
/// don't paint rects we can't see. Without this clip, Cmd+A on a big file
/// allocates 150k `RectOverlay`s every frame and hangs the editor for
/// seconds.
///
/// Change-detection gated: idle frames do nothing.
pub(crate) fn update_selection_highlight(
    mut editor_query: Query<
        (
            Entity,
            EditorRenderView,
            &SelectionState,
            &mut SelectionRects,
            Option<&HiddenLines>,
            Option<&TextBounds>,
            &EditorTheme,
            &SelectionConfig,
        ),
        With<CodeEditor>,
    >,
    dirty_editors: Query<
        Entity,
        (
            With<CodeEditor>,
            Or<(
                Changed<SelectionState>,
                Changed<ScrollPosition>,
                Changed<ComputedNode>,
                Changed<TextBuffer<RopeBuffer>>,
                Changed<FoldState>,
                Changed<MonoCellWidth>,
                Changed<EditorTheme>,
            )>,
        ),
    >,
) {
    let dirty: std::collections::HashSet<Entity> = dirty_editors.iter().collect();
    if dirty.is_empty() {
        return;
    }

    for (
        editor_entity,
        rv,
        sel,
        mut sel_rects,
        hidden,
        wrap,
        theme,
        selection_cfg,
    ) in editor_query.iter_mut()
    {
        if !dirty.contains(&editor_entity) {
            continue;
        }
        sel_rects.0.clear();

        let m = rv.metrics();

        // Visible buffer-line window. Selections are clipped to this band so
        // a multi-thousand-line selection doesn't allocate per-line rects for
        // off-viewport rows.
        let wrap_cfg = wrap.copied().unwrap_or_default();
        let visible = visible_buffer_range(
            &**rv.buffer,
            rv.scroll.y,
            m.viewport_height,
            m.text_area_top,
            m.line_height,
            m.char_width,
            wrap_cfg,
            hidden,
        );
        if visible.start >= visible.end {
            continue;
        }

        // Collect (start_char, end_char) for every active selection range. The
        // SelectionCollection is the single source of truth — emit one rect-set
        // per non-empty selection.
        let selections: Vec<(usize, usize)> = sel
            .selections
            .iter()
            .filter(|s| s.has_selection())
            .map(|s| s.range())
            .collect();

        for (start, end) in selections {
            let sel_start_line = rv.buffer.char_to_line(start);
            let sel_end_line = rv.buffer.char_to_line(end);

            // Iterate only the part of the selection that overlaps the
            // visible window. Off-viewport portions still exist in
            // `SelectionState` — we just don't emit rects for them.
            let iter_start = sel_start_line.max(visible.start);
            let iter_end = sel_end_line.min(visible.end.saturating_sub(1));
            if iter_start > iter_end {
                continue;
            }

            for line_idx in iter_start..=iter_end {
                if rv.fold.is_line_hidden(line_idx) {
                    continue;
                }

                let line_start_char = rv.buffer.line_to_char(line_idx);
                let line = rv.buffer.line(line_idx);
                let line_chars = line.len_chars();

                let sel_start_col = if line_idx == sel_start_line {
                    start - line_start_char
                } else {
                    0
                };
                let sel_end_col = if line_idx == sel_end_line {
                    end - line_start_char
                } else {
                    line_chars
                };
                if sel_start_col >= sel_end_col {
                    continue;
                }

                let s_byte = line.slice(..sel_start_col.min(line_chars)).len_bytes();
                let e_byte = line.slice(..sel_end_col.min(line_chars)).len_bytes();

                push_selection_for_buffer_range(
                    SelSpan {
                        s_byte,
                        e_byte,
                        sel_start_col,
                        sel_end_col,
                        is_last_buffer_line: line_idx == sel_end_line,
                    },
                    &RowMap {
                        layout: rv.layout,
                        fold_state: rv.fold,
                        line_idx,
                    },
                    m.char_width,
                    theme.selection_background,
                    selection_cfg.rounded_selection,
                    &mut sel_rects.0,
                );
            }
        }
    }
}

/// One buffer line's slice of a selection: the byte range, the matching
/// char range (used as a fallback when shaping is unavailable), and a flag
/// marking whether this line is the *last* line of the multi-line selection
/// (the only line that must end at the actual end-x rather than extending to
/// the row's right edge).
struct SelSpan {
    s_byte: usize,
    e_byte: usize,
    sel_start_col: usize,
    sel_end_col: usize,
    is_last_buffer_line: bool,
}

/// Read-only context for mapping `(buffer_row, byte)` → `(display_row, byte_in_row)`.
/// `layout` is `None` for off-viewport buffer lines, in which case the row
/// maps via `fold_state` and pixel math falls back to `char_width`.
struct RowMap<'a> {
    layout: Option<&'a DisplayLayout>,
    fold_state: &'a FoldState,
    /// Buffer line index, in `usize` for `fold_state` lookups. Equals the
    /// `buffer_row` passed to `layout.buffer_to_display` (which takes `u32`).
    line_idx: usize,
}

impl<'a> RowMap<'a> {
    fn buffer_row(&self) -> u32 {
        self.line_idx as u32
    }

    /// Resolve `(display_row, byte_in_row)` for a byte offset within the
    /// buffer line. Fall back to fold-state's display row + raw byte when
    /// the layout doesn't cover this row.
    fn locate(&self, byte_in_line: usize) -> (u32, usize) {
        self.layout
            .and_then(|l| l.buffer_to_display(self.buffer_row(), byte_in_line))
            .unwrap_or_else(|| {
                (
                    self.fold_state.actual_to_display_line(self.line_idx) as u32,
                    byte_in_line,
                )
            })
    }
}

/// Push selection rects for one buffer line's slice. With wrap on, the slice
/// may span multiple display rows; emit one rect per row, extending non-final
/// rows to the row's right edge so the selection band looks continuous.
///
/// Width-fallback note: when a row isn't in `layout.lines` (i.e. shaped) we
/// fall back to `sel_end_col * char_width`. For lines that *are* in the
/// visible buffer-line range but *outside* the layout's narrower shaped
/// slice — which happens at the visible-window edges and during scroll —
/// the caller has the actual selection extent in chars and that's a much
/// better width than a single `char_width`. Without this, boundary lines
/// get a single-char-wide selection rect while shaped lines get the full
/// line rect.
fn push_selection_for_buffer_range(
    span: SelSpan,
    rows: &RowMap<'_>,
    char_width: f32,
    color: Color,
    rounded: bool,
    out: &mut Vec<RectOverlay>,
) {
    let (start_row, start_byte_in_row) = rows.locate(span.s_byte);
    let (end_row, end_byte_in_row) = rows.locate(span.e_byte);

    let start_x = rows
        .layout
        .and_then(|l| l.x_at_byte(start_row, start_byte_in_row))
        .unwrap_or(span.sel_start_col as f32 * char_width);
    // Right edge uses `x_after_source_range` (when start and end are on
    // the same row): if a virtual span — an inlay hint, autosuggest ghost
    // text — is anchored at `end_byte`, `x_at_byte(end_byte)` would jump
    // past it and the selection would visually engulf the virtual
    // decoration. Same-row branch only; multi-row selections fall back to
    // the row-end helper below.
    let end_x_resolved = rows
        .layout
        .and_then(|l| {
            if start_row == end_row {
                l.x_after_source_range(end_row, start_byte_in_row, end_byte_in_row)
            } else {
                l.x_at_byte(end_row, end_byte_in_row)
            }
        })
        .unwrap_or(span.sel_end_col as f32 * char_width);
    // Width fallback for unshaped rows: use the selection extent in chars.
    let end_chars_fallback = span.sel_end_col as f32 * char_width;
    let row_end_or_chars = |row: u32| -> f32 {
        rows.layout
            .and_then(|l| {
                l.lines
                    .iter()
                    .find(|line| line.display_row == row)
                    .and_then(|line| l.x_at_byte(row, line.text.len()))
            })
            .unwrap_or(end_chars_fallback)
    };
    // Non-final-line rows extend to the row's text-end so the selection
    // hugs the actual text instead of filling the row to the viewport edge.
    let trailing_x = if span.is_last_buffer_line {
        end_x_resolved
    } else {
        row_end_or_chars(end_row)
    };

    let corners = if rounded {
        bevy_instanced_text::CornerRadii::uniform(2.0)
    } else {
        bevy_instanced_text::CornerRadii::ZERO
    };

    if start_row == end_row {
        out.push(selection_rect(
            start_row,
            start_x..trailing_x,
            color,
            corners,
        ));
        return;
    }

    let start_row_end = row_end_or_chars(start_row).max(start_x + char_width);
    out.push(selection_rect(
        start_row,
        start_x..start_row_end,
        color,
        corners,
    ));
    for r in (start_row + 1)..end_row {
        let r_end = row_end_or_chars(r).max(char_width);
        out.push(selection_rect(r, 0.0..r_end, color, corners));
    }
    out.push(selection_rect(end_row, 0.0..trailing_x, color, corners));
}

fn selection_rect(
    display_row: u32,
    x_range: std::ops::Range<f32>,
    color: Color,
    corners: bevy_instanced_text::CornerRadii,
) -> RectOverlay {
    RectOverlay {
        display_row,
        x_range,
        vertical: RowVertical::Full,
        color,
        z: -1,
        corners,
    }
}

/// Push a 1-px vertical `RectOverlay` (z = -2) per indent level per
/// visible row, so the engine paints indent guides in the same draw
/// call as the glyphs.
pub(crate) fn update_indent_guides(
    mut editor_query: Query<
        (
            EditorRenderView,
            &EditorTheme,
            &mut IndentGuideRects,
            &Guides,
            &Indentation,
        ),
        With<CodeEditor>,
    >,
) {
    for (rv, theme, mut guide_rects, guides, indentation) in editor_query.iter_mut() {
        if !guides.indentation {
            if !guide_rects.0.is_empty() {
                guide_rects.0.clear();
            }
            continue;
        }

        let m = rv.metrics();
        let indent_size = indentation.tab_size as usize;

        let mut new_rects: Vec<RectOverlay> = Vec::new();
        for (buffer_line, display_row) in
            visible_display_rows(rv.fold, rv.buffer.len_lines(), m.line_height, rv.scroll.y, m.viewport_height)
        {
            let line = rv.buffer.line(buffer_line);
            let mut leading_spaces = 0;
            for c in line.chars() {
                match c {
                    ' ' => leading_spaces += 1,
                    '\t' => leading_spaces += indent_size,
                    _ => break,
                }
            }
            let indent_levels = leading_spaces / indent_size;

            for level in 0..indent_levels {
                let x = (level * indent_size) as f32 * m.char_width;
                new_rects.push(RectOverlay {
                    display_row: display_row as u32,
                    x_range: x..(x + 1.0),
                    vertical: RowVertical::FullLeaded,
                    color: theme.indent_guide,
                    z: 0,
                    corners: bevy_instanced_text::CornerRadii::ZERO,
                });
            }
        }

        if guide_rects.0 != new_rects {
            guide_rects.0 = new_rects;
        }
    }
}

/// Push a 1-px vertical `RectOverlay` per ruler column per visible row.
pub(crate) fn update_rulers(
    mut editor_query: Query<
        (
            EditorRenderView,
            &EditorTheme,
            &mut RulerRects,
            &Rulers,
        ),
        With<CodeEditor>,
    >,
) {
    for (rv, theme, mut ruler_rects, rulers) in editor_query.iter_mut() {
        if rulers.0.is_empty() {
            if !ruler_rects.0.is_empty() {
                ruler_rects.0.clear();
            }
            continue;
        }

        let m = rv.metrics();

        let mut new_rects: Vec<RectOverlay> = Vec::new();
        for (_buffer_line, display_row) in
            visible_display_rows(rv.fold, rv.buffer.len_lines(), m.line_height, rv.scroll.y, m.viewport_height)
        {
            for ruler in &rulers.0 {
                let x = ruler.column as f32 * m.char_width;
                new_rects.push(RectOverlay {
                    display_row: display_row as u32,
                    x_range: x..(x + 1.0),
                    vertical: RowVertical::FullLeaded,
                    color: ruler.color.unwrap_or(theme.indent_guide),
                    z: 0,
                    corners: bevy_instanced_text::CornerRadii::ZERO,
                });
            }
        }

        if ruler_rects.0 != new_rects {
            ruler_rects.0 = new_rects;
        }
    }
}

/// Push a 1-px-tall underline at the bottom of every folded region's
/// visible (placeholder) row when `Folding::highlight` is enabled.
///
/// The underline spans the full viewport width so the band reads as a
/// continuous boundary even when the folded line is short.
pub(crate) fn update_fold_highlights(
    mut editor_query: Query<
        (
            EditorRenderView,
            &EditorTheme,
            &mut FoldHighlightRects,
            &Folding,
        ),
        With<CodeEditor>,
    >,
) {
    for (rv, theme, mut fold_rects, folding) in editor_query.iter_mut() {
        if !folding.highlight {
            if !fold_rects.0.is_empty() {
                fold_rects.0.clear();
            }
            continue;
        }

        let folded_regions: Vec<usize> = rv
            .fold
            .regions
            .iter()
            .filter(|r| r.is_folded)
            .map(|r| r.start_line)
            .collect();

        if folded_regions.is_empty() {
            if !fold_rects.0.is_empty() {
                fold_rects.0.clear();
            }
            continue;
        }

        let m = rv.metrics();

        let mut new_rects: Vec<RectOverlay> = Vec::with_capacity(folded_regions.len());
        for start_line in folded_regions {
            let display_row = rv.fold.actual_to_display_line(start_line) as u32;
            new_rects.push(RectOverlay {
                display_row,
                x_range: 0.0..m.viewport_width,
                vertical: RowVertical::Full,
                color: theme.fold_marker,
                z: -1,
                corners: bevy_instanced_text::CornerRadii::ZERO,
            });
        }

        if fold_rects.0 != new_rects {
            fold_rects.0 = new_rects;
        }
    }
}

/// Push tiny `RectOverlay`s marking whitespace characters according to
/// [`RenderWhitespace`]. Spaces render as a centered dot (narrow x range,
/// `Caret { 0.15 }` vertical), tabs as a thin horizontal bar through the
/// row's midline.
///
/// Modes:
/// - `None`: no markers.
/// - `Boundary`: leading whitespace and trailing whitespace on each line.
/// - `Selection`: only whitespace inside the active selection range(s).
/// - `Trailing`: only trailing whitespace on each line.
/// - `All`: every whitespace character.
///
/// Tab columns are computed by walking the line and snapping to the next
/// `indentation.tab_size` boundary so the marker bar spans the actual
/// tab-stop width, not a fixed char cell.
pub(crate) fn update_whitespace_markers(
    mut editor_query: Query<
        (
            EditorRenderView,
            Option<&HiddenLines>,
            Option<&TextBounds>,
            &EditorTheme,
            &RenderSettings,
            &SelectionState,
            &Indentation,
            &mut WhitespaceRects,
        ),
        With<CodeEditor>,
    >,
) {
    for (rv, hidden, bounds, theme, render, sel, indentation, mut rects) in
        editor_query.iter_mut()
    {
        if matches!(render.render_whitespace, RenderWhitespace::None) {
            if !rects.0.is_empty() {
                rects.0.clear();
            }
            continue;
        }

        let m = rv.metrics();
        let wrap_cfg = bounds.copied().unwrap_or_default();
        let visible = visible_buffer_range(
            &**rv.buffer,
            rv.scroll.y,
            m.viewport_height,
            m.text_area_top,
            m.line_height,
            m.char_width,
            wrap_cfg,
            hidden,
        );

        let tab_size = indentation.tab_size.max(1) as usize;

        let mut sel_ranges: Vec<(usize, usize)> = Vec::new();
        if matches!(render.render_whitespace, RenderWhitespace::Selection) {
            sel_ranges = sel
                .selections
                .iter()
                .filter(|s| s.has_selection())
                .map(|s| s.range())
                .collect();
            if sel_ranges.is_empty() {
                if !rects.0.is_empty() {
                    rects.0.clear();
                }
                continue;
            }
        }

        let mut new_rects: Vec<RectOverlay> = Vec::new();

        if visible.start < visible.end {
            for buffer_line in visible.start..visible.end {
                if rv.fold.is_line_hidden(buffer_line) {
                    continue;
                }
                let line = rv.buffer.line(buffer_line);
                let line_start_char = rv.buffer.line_to_char(buffer_line);
                let line_chars = line.len_chars();
                if line_chars == 0 {
                    continue;
                }

                let chars: Vec<char> = line.chars().collect();
                let trim_end_col = line_trim_end_col(&chars);
                let leading_end_col = line_leading_ws_end_col(&chars);

                let display_row = rv.fold.actual_to_display_line(buffer_line) as u32;

                let mut visual_col = 0usize;
                for (col, ch) in chars.iter().enumerate() {
                    if *ch == '\n' || *ch == '\r' {
                        break;
                    }
                    let is_space = *ch == ' ';
                    let is_tab = *ch == '\t';
                    if !is_space && !is_tab {
                        visual_col += 1;
                        continue;
                    }
                    let tab_advance = if is_tab {
                        tab_size - (visual_col % tab_size)
                    } else {
                        1
                    };
                    let show = match render.render_whitespace {
                        RenderWhitespace::None => false,
                        RenderWhitespace::All => true,
                        RenderWhitespace::Trailing => col >= trim_end_col,
                        RenderWhitespace::Boundary => col < leading_end_col || col >= trim_end_col,
                        RenderWhitespace::Selection => {
                            let abs = line_start_char + col;
                            sel_ranges.iter().any(|(s, e)| abs >= *s && abs < *e)
                        }
                    };
                    if show {
                        let x_start = visual_col as f32 * m.char_width;
                        if is_space {
                            let dot_half = (m.char_width * 0.07).max(0.4);
                            let center = x_start + m.char_width * 0.5;
                            new_rects.push(RectOverlay {
                                display_row,
                                x_range: (center - dot_half)..(center + dot_half),
                                vertical: RowVertical::Caret {
                                    height_fraction: 0.1,
                                },
                                color: theme.whitespace,
                                z: 0,
                                corners: bevy_instanced_text::CornerRadii::ZERO,
                            });
                        } else {
                            let x_end = (visual_col + tab_advance) as f32 * m.char_width;
                            let pad = m.char_width * 0.25;
                            new_rects.push(RectOverlay {
                                display_row,
                                x_range: (x_start + pad)..(x_end - pad).max(x_start + pad + 1.0),
                                vertical: RowVertical::Strikethrough { thickness: 0.8 },
                                color: theme.whitespace,
                                z: 0,
                                corners: bevy_instanced_text::CornerRadii::ZERO,
                            });
                        }
                    }
                    visual_col += tab_advance;
                }
            }
        }

        if rects.0 != new_rects {
            rects.0 = new_rects;
        }
    }
}

fn line_trim_end_col(chars: &[char]) -> usize {
    let mut end = chars.len();
    while end > 0 {
        let c = chars[end - 1];
        if c == '\n' || c == '\r' || c == ' ' || c == '\t' {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

fn line_leading_ws_end_col(chars: &[char]) -> usize {
    let mut col = 0usize;
    while col < chars.len() {
        let c = chars[col];
        if c == ' ' || c == '\t' {
            col += 1;
        } else {
            break;
        }
    }
    col
}

/// Run condition: auto-scroll only fires for editors that have moved their
/// cursor and aren't currently being mouse-dragged.
///
/// Drag suppression is per-entity (Component) — dragging in editor A no
/// longer blocks auto-scroll in editor B (the previous global Resource shape).
pub(crate) fn should_auto_scroll(
    editor_query: Query<
        (
            EditorBufferView,
            &CursorState,
            &bevy_instanced_text_editor::TextViewDragState,
        ),
        With<CodeEditor>,
    >,
) -> bool {
    for (buf, cursor, mouse_drag) in editor_query.iter() {
        if mouse_drag.is_dragging {
            continue;
        }
        let cursor_pos = cursor.cursor_pos.min(buf.buffer.len_chars());
        if cursor_pos != cursor.last_cursor_pos {
            return true;
        }
    }
    false
}

pub(crate) fn auto_scroll_to_cursor(mut editor_query: AutoScrollQuery) {
    for (rv, mut scroll_target) in editor_query.iter_mut() {
        let cursor_pos = scroll_target.cursor.cursor_pos.min(rv.buffer.len_chars());
        scroll_target.cursor.last_cursor_pos = cursor_pos;
        let m = rv.metrics();
        if m.viewport_height < 1.0 || m.line_height < 1.0 {
            continue;
        }
        let line_index = rv.buffer.char_to_line(cursor_pos);

        let mut target =
            if scroll_target.animator.target == Vec2::ZERO && rv.scroll.0 != Vec2::ZERO {
                rv.scroll.0
            } else {
                scroll_target.animator.target
            };
        let cursor_y = m.text_area_top - target.y + line_index as f32 * m.line_height;

        let margin_vertical = m.line_height * 2.0;
        let visible_top = margin_vertical;
        let visible_bottom = m.viewport_height - margin_vertical;

        let mut vertical_changed = true;
        if cursor_y < visible_top {
            target.y -= visible_top - cursor_y;
        } else if cursor_y > visible_bottom {
            target.y += cursor_y - visible_bottom;
        } else {
            vertical_changed = false;
        }

        if vertical_changed {
            let line_count = rv.buffer.len_lines();
            let content_height = line_count as f32 * m.line_height;
            let max_scroll = (content_height - m.viewport_height + m.text_area_top).max(0.0);
            target.y = target.y.clamp(0.0, max_scroll);
        }

        let wrap_on = scroll_target.bounds.is_some_and(|b| b.width.is_some());
        if wrap_on {
            target.x = 0.0;
        } else {
            let line_start = rv.buffer.line_to_char(line_index);
            let col_index = cursor_pos - line_start;
            let cursor_x = col_index as f32 * m.char_width;

            let margin_horizontal = scroll_target
                .scroll_cfg
                .reveal_horizontal_right_padding
                .max(m.char_width);
            let visible_left = target.x;
            let visible_right = target.x + m.viewport_width - m.text_area_left - margin_horizontal;

            if cursor_x < visible_left {
                target.x = cursor_x.max(0.0);
            } else if cursor_x > visible_right {
                target.x = cursor_x - (m.viewport_width - m.text_area_left - margin_horizontal);
            }

            let max_horizontal_scroll =
                (scroll_target.metrics.max_content_width - m.viewport_width).max(0.0);
            target.x = target.x.clamp(0.0, max_horizontal_scroll);
        }

        if !vertical_changed && (target.x - scroll_target.animator.target.x).abs() < f32::EPSILON {
            continue;
        }
        scroll_target.animator.target = target;
    }
}
