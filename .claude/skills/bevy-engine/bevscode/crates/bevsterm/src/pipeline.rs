//! Wezterm grid → `TextBuffer<TextSpan>` (string buffer) + per-line `LineStyles`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};

use bevy_instanced_text::{
    FormattedSpan, LineStyles, MonoCellWidth, TextBackgroundColor, TextBuffer, TextColor,
    TextFormat, TextSpan,
};
use wezterm_surface::SequenceNo;
use wezterm_term::Line as VtLine;

use crate::backend::{ColorAttribute, CursorVisibility, Intensity, Underline as VtUnderline};
use crate::text::{
    TerminalColorPalette, TerminalGridSnapshot, TerminalScrollFollow, TerminalSession,
};

#[derive(Clone)]
struct CachedLine {
    text: String,
    runs: Vec<FormattedSpan>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct RebuildCache {
    last_seqno: Option<SequenceNo>,
    last_rows: usize,
    last_cols: usize,
    last_total_lines: usize,
    lines: Vec<CachedLine>,
}

type SnapshotQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TerminalSession,
        &'static mut TextBuffer<TextSpan>,
        &'static ComputedNode,
        &'static TextFont,
        &'static bevy::text::LineHeight,
        &'static MonoCellWidth,
        &'static mut ScrollPosition,
        &'static TerminalColorPalette,
        &'static TextColor,
        &'static TextBackgroundColor,
        &'static mut LineStyles,
        &'static mut TerminalGridSnapshot,
        &'static mut TerminalScrollFollow,
        &'static mut crate::text::ScrollFollowState,
    ),
>;

pub(crate) fn sync_grid_snapshot(
    mut q: SnapshotQuery,
    mut cache: Local<HashMap<Entity, RebuildCache>>,
) {
    cache.retain(|e, _| q.contains(*e));
    for (
        entity,
        session,
        mut buffer,
        computed,
        font,
        lh,
        _mono,
        mut scroll,
        palette,
        fg_color,
        bg_color,
        mut line_styles,
        mut snapshot,
        mut follow,
        mut follow_state,
    ) in q.iter_mut()
    {
        let line_height = bevy_instanced_text::resolve_line_height(*lh, font.font_size);
        let cache_entry = cache.entry(entity).or_default();

        let term = session.terminal.lock();
        let screen = term.screen();
        let cols = screen.physical_cols;
        let rows = screen.physical_rows;
        let total_lines = screen.scrollback_rows();
        let scrollback_offset = total_lines.saturating_sub(rows);
        let seqno = term.current_seqno();
        let needs_rebuild = cache_entry.last_seqno != Some(seqno) || cache_entry.last_rows != rows;

        if !needs_rebuild {
            anchor_scroll_to_bottom(
                &mut scroll,
                computed,
                line_height,
                total_lines,
                &mut follow,
                &mut follow_state,
            );
            continue;
        }

        let cache_valid = cache_entry.last_seqno.is_some()
            && cache_entry.last_cols == cols
            && cache_entry.last_total_lines == total_lines
            && cache_entry.lines.len() == total_lines;
        let prev_seqno = cache_entry.last_seqno.unwrap_or(0);

        let mut text = String::with_capacity(total_lines * (cols + 1));
        let mut by_line: HashMap<u32, Vec<FormattedSpan>> = HashMap::with_capacity(total_lines);
        let mut next_lines: Vec<CachedLine> = Vec::with_capacity(total_lines);

        screen.for_each_phys_line(|phys_y, line| {
            if cache_valid && !line.changed_since(prev_seqno) {
                let cached = &cache_entry.lines[phys_y];
                text.push_str(&cached.text);
                text.push('\n');
                by_line.insert(phys_y as u32, cached.runs.clone());
                next_lines.push(cached.clone());
                return;
            }

            let (line_text, runs) = shape_phys_line(line, cols, palette, fg_color, bg_color);
            text.push_str(&line_text);
            text.push('\n');
            by_line.insert(phys_y as u32, runs.clone());
            next_lines.push(CachedLine {
                text: line_text,
                runs,
            });
        });

        if buffer.0 .0 != text {
            buffer.0 = TextSpan(text);
        }

        *line_styles = LineStyles::new(by_line);

        let cursor = term.cursor_pos();
        drop(term);

        snapshot.version = snapshot.version.wrapping_add(1);
        snapshot.cols = cols as u16;
        snapshot.rows = rows as u16;
        let cursor_row_in_buffer = scrollback_offset as u32 + cursor.y.max(0) as u32;
        let max_row = total_lines.saturating_sub(1) as u32;
        let max_col = (cols as u16).saturating_sub(1);
        snapshot.cursor_row = cursor_row_in_buffer.min(max_row);
        snapshot.cursor_col = (cursor.x as u16).min(max_col);
        snapshot.cursor_hidden = matches!(cursor.visibility, CursorVisibility::Hidden);

        cache_entry.last_seqno = Some(seqno);
        cache_entry.last_rows = rows;
        cache_entry.last_cols = cols;
        cache_entry.last_total_lines = total_lines;
        cache_entry.lines = next_lines;
        anchor_scroll_to_bottom(
            &mut scroll,
            computed,
            line_height,
            total_lines,
            &mut follow,
            &mut follow_state,
        );
    }
}

fn anchor_scroll_to_bottom(
    scroll: &mut ScrollPosition,
    computed: &ComputedNode,
    line_height: f32,
    total_lines: usize,
    follow: &mut TerminalScrollFollow,
    follow_state: &mut crate::text::ScrollFollowState,
) {
    if line_height <= 0.0 {
        return;
    }
    let inv = computed.inverse_scale_factor();
    let viewport_height = computed.size().y * inv;
    let text_area_top = computed.content_inset().min_inset.y * inv;
    let visible_rows = ((viewport_height - text_area_top) / line_height)
        .floor()
        .max(0.0) as usize;
    let hidden_rows = total_lines.saturating_sub(visible_rows);
    let max_scroll = hidden_rows as f32 * line_height;
    let stick_threshold = line_height;

    if (scroll.y - follow_state.last_applied_target).abs() > 0.5 {
        follow.stick_to_bottom = max_scroll - scroll.y <= stick_threshold;
    }

    if follow.stick_to_bottom || max_scroll - scroll.y <= stick_threshold {
        follow.stick_to_bottom = true;
        scroll.y = max_scroll;
        follow_state.last_applied_target = max_scroll;
    } else {
        follow_state.last_applied_target = scroll.y;
    }
}

fn shape_phys_line(
    line: &VtLine,
    cols: usize,
    palette: &TerminalColorPalette,
    fg: &TextColor,
    bg: &TextBackgroundColor,
) -> (String, Vec<FormattedSpan>) {
    let mut line_text = String::with_capacity(cols);
    let mut runs: Vec<FormattedSpan> = Vec::new();
    let mut current: Option<(TextFormat, String)> = None;

    for cell in line.visible_cells() {
        let cell_str = cell.str();
        let ch = cell_str.chars().next().unwrap_or(' ');
        line_text.push(ch);

        let attrs = cell.attrs();
        let fg_color = resolve_color(attrs.foreground(), palette, fg, bg, true);
        let bg_color = match attrs.background() {
            ColorAttribute::Default => None,
            other => Some(resolve_color(other, palette, fg, bg, false)),
        };

        let run_proto = TextFormat {
            byte_range: 0..0,
            fg: fg_color,
            bg: bg_color,
            font_scale: 1.0,
            skew: 0.0,
            corner_radius: 0.0,
            font_weight: match attrs.intensity() {
                Intensity::Bold => Some(700),
                Intensity::Half => Some(300),
                Intensity::Normal => None,
            },
            italic: attrs.italic(),
            font: None,
            decoration: {
                let mut d = bevy_instanced_text::TextDecoration::empty();
                if !matches!(attrs.underline(), VtUnderline::None) {
                    d |= bevy_instanced_text::TextDecoration::UNDERLINE;
                }
                if attrs.strikethrough() {
                    d |= bevy_instanced_text::TextDecoration::STRIKETHROUGH;
                }
                d
            },
            link: None,
        };

        match current.as_mut() {
            Some((prev, buf)) if style_run_matches(prev, &run_proto) => buf.push(ch),
            _ => {
                if let Some((format, buf)) = current.take() {
                    runs.push(FormattedSpan {
                        text: buf,
                        format,
                        is_virtual: false,
                    });
                }
                let mut buf = String::new();
                buf.push(ch);
                current = Some((run_proto, buf));
            }
        }
    }
    while line_text.chars().count() < cols {
        line_text.push(' ');
    }
    if let Some((format, buf)) = current.take() {
        runs.push(FormattedSpan {
            text: buf,
            format,
            is_virtual: false,
        });
    }
    (line_text, runs)
}

fn style_run_matches(a: &TextFormat, b: &TextFormat) -> bool {
    a.fg == b.fg
        && a.bg == b.bg
        && a.font_weight == b.font_weight
        && a.italic == b.italic
        && a.decoration == b.decoration
}

fn resolve_color(
    color: ColorAttribute,
    palette: &TerminalColorPalette,
    fg: &TextColor,
    bg: &TextBackgroundColor,
    is_fg: bool,
) -> Color {
    match color {
        ColorAttribute::Default => {
            if is_fg {
                **fg
            } else {
                **bg
            }
        }
        ColorAttribute::PaletteIndex(idx) => {
            if (idx as usize) < palette.ansi.len() {
                palette.ansi[idx as usize]
            } else {
                indexed_to_color(idx)
            }
        }
        ColorAttribute::TrueColorWithPaletteFallback(color, _)
        | ColorAttribute::TrueColorWithDefaultFallback(color) => {
            Color::srgba(color.0, color.1, color.2, color.3)
        }
    }
}

fn indexed_to_color(idx: u8) -> Color {
    if idx < 16 {
        return Color::srgb(0.5, 0.5, 0.5);
    }
    if idx < 232 {
        let n = idx - 16;
        let r = (n / 36) % 6;
        let g = (n / 6) % 6;
        let b = n % 6;
        let to_f = |c: u8| {
            if c == 0 {
                0.0
            } else {
                (40.0 * c as f32 + 55.0) / 255.0
            }
        };
        return Color::srgb(to_f(r), to_f(g), to_f(b));
    }
    let step = idx - 232;
    let v = (8.0 + 10.0 * step as f32) / 255.0;
    Color::srgb(v, v, v)
}
