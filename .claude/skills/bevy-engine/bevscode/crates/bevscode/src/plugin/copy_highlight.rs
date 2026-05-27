//! Bevscode-side copy override: when
//! [`SelectionConfig::copy_with_syntax_highlighting`] is on, format the
//! current selection as HTML using the syntax-highlighter and write that
//! to the clipboard alongside the plain-text fallback.
//!
//! Runs *after* the editor crate's `handle_copy` (same `Update` schedule,
//! same `CopyRequested` event read independently). The editor crate has
//! already written plain text by the time this fires; this system either
//! leaves that text alone (setting disabled / no provider / empty
//! selection) or replaces it with `set_html(html, alt_text)`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text::TextBuffer;
use bevy_instanced_text_editor::{ClipboardResource, CopyRequested, RopeBuffer, SelectionState};

use crate::plugin::syntax_highlighting::EditorSyntaxState;
use crate::settings::{EditorTheme, SelectionConfig, SyntaxColors};
use crate::types::{CodeEditor, LineSegment};

pub(crate) fn handle_copy_with_highlighting(
    mut events: MessageReader<CopyRequested>,
    input_focus: Res<InputFocus>,
    clipboard: Res<ClipboardResource>,
    q: Query<
        (
            &SelectionState,
            &TextBuffer<RopeBuffer>,
            &SelectionConfig,
            &SyntaxColors,
            &EditorTheme,
            Option<&bevy_tree_sitter::SyntaxTree>,
            &mut EditorSyntaxState,
        ),
        With<CodeEditor>,
    >,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((sel, buffer, cfg, syntax_colors, theme, syntax_tree, mut syntax_state)) =
        q.get_inner(entity)
    else {
        return;
    };
    if !cfg.copy_with_syntax_highlighting {
        return;
    }
    let Some((start, end)) = sel.primary_range() else {
        return;
    };
    let start = start.min(buffer.len_chars());
    let end = end.min(buffer.len_chars());
    if start == end {
        return;
    }
    let plain = buffer.slice(start..end).to_string();
    let Some(syntax_tree) = syntax_tree else {
        return;
    };
    if !syntax_state.is_available() {
        return;
    }

    let start_line = buffer.char_to_line(start);
    let end_line = buffer.char_to_line(end);

    let line_strings: Vec<String> = (start_line..=end_line)
        .map(|li| buffer.line(li).to_string())
        .collect();
    let line_inputs: Vec<(usize, &str)> = (start_line..=end_line)
        .zip(line_strings.iter())
        .map(|(li, s)| (buffer.rope().line_to_byte(li), s.as_str()))
        .collect();

    let styled = syntax_state.highlight_lines(
        &line_inputs,
        syntax_tree,
        buffer.rope(),
        syntax_colors,
        theme.foreground,
    );

    let bg = color_to_hex(theme.background);
    let fg = color_to_hex(theme.foreground);
    let mut html = String::with_capacity(plain.len() * 4);
    html.push_str(&format!(
        "<pre style=\"font-family:monospace;background:{bg};color:{fg};padding:8px;\">",
    ));

    for (idx, (line_idx, segments)) in (start_line..=end_line).zip(styled.iter()).enumerate() {
        let line_start_char = buffer.line_to_char(line_idx);
        let line_chars = buffer.line(line_idx).len_chars();
        let line_text = &line_strings[idx];

        let sel_start_in_line = if line_idx == start_line {
            start - line_start_char
        } else {
            0
        };
        let sel_end_in_line = if line_idx == end_line {
            end - line_start_char
        } else {
            line_chars
        };

        let line_no_nl = line_text.strip_suffix('\n').unwrap_or(line_text);
        emit_line_html(
            segments,
            line_no_nl,
            sel_start_in_line,
            sel_end_in_line,
            &mut html,
        );

        if idx + 1 < styled.len() {
            html.push('\n');
        }
    }
    html.push_str("</pre>");

    clipboard.set_html(&html, &plain);
}

fn emit_line_html(
    segments: &[LineSegment],
    line: &str,
    sel_start_col: usize,
    sel_end_col: usize,
    out: &mut String,
) {
    if segments.is_empty() {
        let slice: String = line
            .chars()
            .skip(sel_start_col)
            .take(sel_end_col.saturating_sub(sel_start_col))
            .collect();
        push_escaped(&slice, out);
        return;
    }
    let mut col = 0usize;
    for seg in segments {
        let seg_chars = seg.text.chars().count();
        let seg_start = col;
        let seg_end = col + seg_chars;
        col = seg_end;

        let clip_start = seg_start.max(sel_start_col);
        let clip_end = seg_end.min(sel_end_col);
        if clip_start >= clip_end {
            continue;
        }
        let take = clip_end - clip_start;
        let skip = clip_start - seg_start;
        let visible: String = seg.text.chars().skip(skip).take(take).collect();
        if visible.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "<span style=\"color:{}\">",
            color_to_hex(seg.color)
        ));
        push_escaped(&visible, out);
        out.push_str("</span>");
    }
}

fn push_escaped(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

fn color_to_hex(c: Color) -> String {
    let s = c.to_srgba();
    let r = (s.red.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (s.green.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (s.blue.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}
