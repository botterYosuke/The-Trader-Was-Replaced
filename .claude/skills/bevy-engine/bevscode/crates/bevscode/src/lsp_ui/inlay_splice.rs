use std::collections::HashMap;

use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;

use crate::text_view::TextBuffer;
use crate::types::CodeEditor;

use super::components::{InlayHintData, InlayHintKind};
use super::state::LspInlayHints;

/// Splice each editor's `InlayHintData` entities into its `LineStyles` so
/// the engine's shape pipeline lays the hints out inline (subsequent source
/// glyphs shift right). Runs after `produce_line_styles` so it sees the
/// freshly-written, inlay-free per-line span vec; rebuilds with virtual
/// spans inserted at the right byte offsets.
///
/// **Idempotent**: drops any pre-existing virtual spans before splicing so
/// re-running on top of its own output produces the same result. This
/// matters because we mutate `LineStyles` in place and Bevy doesn't roll
/// back the previous frame's write.
///
/// Encoding: `InlayHintData.character` is in the LSP server's negotiated
/// position encoding (UTF-16 code units by default). We convert via
/// `bevy_lsp::lsp_position_to_rope_byte` using the editor's
/// `ServerCapabilities::position_encoding()`.
pub fn splice_inlays_into_line_styles(
    mut editors: Query<
        (
            &TextBuffer<RopeBuffer>,
            &mut bevy_instanced_text::LineStyles,
            &bevy_lsp::ServerCapabilities,
            Ref<LspInlayHints>,
        ),
        With<CodeEditor>,
    >,
    hints: Query<&InlayHintData>,
    theme: Res<crate::lsp_ui_tempera::inline_decorations::InlineDecorationsTheme>,
) {
    for (buffer, mut line_styles, caps, hint_state) in editors.iter_mut() {
        // Only run when something that could affect the splice changed:
        // hint state arrived, or LineStyles got a fresh write whose
        // virtuals we need to re-insert. `line_styles.is_changed()` also
        // returns true on the frame after our own write, but that pass
        // will find no diff vs the cached map and bail without mutating.
        if !hint_state.is_changed() && !line_styles.is_changed() {
            continue;
        }

        // Group inlays by line, keyed by source byte offset in that line
        // (post UTF-16 → byte conversion). Sorted ascending so the
        // splice walk is monotonic.
        let rope = buffer.rope();
        let enc = caps.position_encoding();
        let mut by_line: HashMap<u32, Vec<(usize, &InlayHintData)>> = HashMap::new();
        for hint in hints.iter() {
            if (hint.line as usize) >= rope.len_lines() {
                continue;
            }
            let line_byte_start = rope.line_to_byte(hint.line as usize);
            let pos = lsp_types::Position {
                line: hint.line,
                character: hint.character,
            };
            let rope_byte = bevy_lsp::lsp_position_to_rope_byte(rope, pos, enc);
            let byte_in_line = rope_byte.saturating_sub(line_byte_start);
            by_line
                .entry(hint.line)
                .or_default()
                .push((byte_in_line, hint));
        }
        for v in by_line.values_mut() {
            v.sort_by_key(|(b, _)| *b);
        }

        let mut next = (*line_styles.by_line).clone();
        let mut changed = false;

        for (line, spans) in next.iter_mut() {
            // Drop any virtuals from a prior splice on this line.
            let before_len = spans.len();
            spans.retain(|s| !s.is_virtual);
            if spans.len() != before_len {
                changed = true;
            }

            let Some(inlays) = by_line.get(line) else {
                continue;
            };
            if inlays.is_empty() {
                continue;
            }
            changed = true;
            *spans = splice_inlays_into_spans(spans, inlays, &theme);
        }

        if changed {
            *line_styles = bevy_instanced_text::LineStyles::new(next);
        }
    }
}

/// Splice virtual inlay spans into `source_spans` at the byte offsets in
/// `inlays` (sorted ascending by byte). Returns a new span vec where each
/// inlay sits between source bytes — splitting a source span if the inlay
/// lands mid-span.
fn splice_inlays_into_spans(
    source_spans: &[bevy_instanced_text::FormattedSpan],
    inlays: &[(usize, &InlayHintData)],
    theme: &crate::lsp_ui_tempera::inline_decorations::InlineDecorationsTheme,
) -> Vec<bevy_instanced_text::FormattedSpan> {
    let mut out: Vec<bevy_instanced_text::FormattedSpan> =
        Vec::with_capacity(source_spans.len() + inlays.len());
    let mut inlay_iter = inlays.iter().peekable();
    let mut byte_cursor: usize = 0;

    for span in source_spans {
        let span_start = byte_cursor;
        let span_end = span_start + span.text.len();
        // Cut points for any inlays that fall within (`< span_end`) the
        // current span. An inlay at exactly `span_end` belongs at the
        // start of the *next* span.
        let mut last_cut = 0usize;
        while let Some((byte_in_line, _)) = inlay_iter.peek() {
            if *byte_in_line >= span_end {
                break;
            }
            let (byte_in_line, hint) = inlay_iter.next().unwrap();
            let local = byte_in_line.saturating_sub(span_start);
            if local > last_cut {
                out.push(clone_with_text(span, &span.text[last_cut..local]));
            }
            out.push(inlay_span(hint, theme));
            last_cut = local;
        }
        if last_cut < span.text.len() {
            out.push(clone_with_text(span, &span.text[last_cut..]));
        }
        byte_cursor = span_end;
    }
    // Inlays past the last source byte sit at end-of-line (e.g. trailing
    // type annotations on lines that end before the hint's reported byte).
    for (_, hint) in inlay_iter {
        out.push(inlay_span(hint, theme));
    }
    out
}

fn clone_with_text(
    src: &bevy_instanced_text::FormattedSpan,
    text: &str,
) -> bevy_instanced_text::FormattedSpan {
    bevy_instanced_text::FormattedSpan {
        text: text.to_string(),
        format: src.format.clone(),
        is_virtual: false,
    }
}

/// Build the virtual `FormattedSpan` for a single inlay hint.
fn inlay_span(
    hint: &InlayHintData,
    theme: &crate::lsp_ui_tempera::inline_decorations::InlineDecorationsTheme,
) -> bevy_instanced_text::FormattedSpan {
    let color = match hint.kind {
        InlayHintKind::Type => theme.inlay_type,
        InlayHintKind::Parameter => theme.inlay_parameter,
        InlayHintKind::Other => theme.inlay_other,
    };
    let mut format = bevy_instanced_text::TextFormat::fg(0..0, color);
    if theme.inlay_font_scale != 1.0 {
        format = format.with_scale(theme.inlay_font_scale);
    }
    bevy_instanced_text::FormattedSpan {
        text: hint.label.clone(),
        format,
        is_virtual: true,
    }
}

#[cfg(test)]
mod inlay_splice_tests {
    //! Unit tests for the inlay-hint splice logic.
    //!
    //! `splice_inlays_into_spans` is the meat of the splicer — it takes
    //! source-spans + a per-line list of inlays and returns interleaved
    //! spans where each inlay sits at the right byte offset, splitting
    //! source spans that an inlay falls inside. Tests target this
    //! function directly to keep the harness light; the outer system is
    //! straightforward wiring (group by line, drop prior virtuals, call
    //! this).
    use super::*;
    use crate::lsp_ui_tempera::inline_decorations::InlineDecorationsTheme;
    use bevy_instanced_text::{FormattedSpan, TextFormat};

    fn src(text: &str) -> FormattedSpan {
        FormattedSpan {
            text: text.to_string(),
            format: TextFormat::fg(0..0, bevy::prelude::Color::WHITE),
            is_virtual: false,
        }
    }

    fn hint(line: u32, character: u32, label: &str, kind: InlayHintKind) -> InlayHintData {
        InlayHintData {
            line,
            character,
            label: label.to_string(),
            kind,
        }
    }

    /// An inlay at the boundary between two source spans inserts cleanly
    /// between them without splitting either.
    #[test]
    fn inlay_at_span_boundary_inserts_between() {
        let theme = InlineDecorationsTheme::default();
        let spans = vec![src("let x"), src(" = foo()")];
        // Inlay at byte 5 (start of " = foo()" — right at the boundary).
        let h = hint(0, 5, ": i32", InlayHintKind::Type);
        let inlays = vec![(5usize, &h)];

        let out = splice_inlays_into_spans(&spans, &inlays, &theme);

        assert_eq!(out.len(), 3, "expected source + virtual + source");
        assert_eq!(out[0].text, "let x");
        assert!(!out[0].is_virtual);
        assert_eq!(out[1].text, ": i32");
        assert!(out[1].is_virtual);
        assert_eq!(out[2].text, " = foo()");
        assert!(!out[2].is_virtual);
    }

    /// An inlay landing inside a source span splits the span into a
    /// prefix + the virtual + a suffix, each keyed off the original
    /// source span's style.
    #[test]
    fn inlay_inside_span_splits_it() {
        let theme = InlineDecorationsTheme::default();
        let spans = vec![src("foo(bar)")];
        // Inlay at byte 4 (between `(` and `bar`).
        let h = hint(0, 4, "param: ", InlayHintKind::Parameter);
        let inlays = vec![(4usize, &h)];

        let out = splice_inlays_into_spans(&spans, &inlays, &theme);

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].text, "foo(");
        assert!(!out[0].is_virtual);
        assert_eq!(out[1].text, "param: ");
        assert!(out[1].is_virtual);
        assert_eq!(out[2].text, "bar)");
        assert!(!out[2].is_virtual);
    }

    /// Multiple inlays on one line interleave in sorted byte order.
    #[test]
    fn multiple_inlays_interleave_in_order() {
        let theme = InlineDecorationsTheme::default();
        let spans = vec![src("abcde")];
        let h1 = hint(0, 1, "[1]", InlayHintKind::Type);
        let h2 = hint(0, 3, "[2]", InlayHintKind::Type);
        let inlays = vec![(1usize, &h1), (3usize, &h2)];

        let out = splice_inlays_into_spans(&spans, &inlays, &theme);

        let texts: Vec<&str> = out.iter().map(|s| s.text.as_str()).collect();
        let virtuals: Vec<bool> = out.iter().map(|s| s.is_virtual).collect();
        assert_eq!(texts, vec!["a", "[1]", "bc", "[2]", "de"]);
        assert_eq!(virtuals, vec![false, true, false, true, false]);
    }

    /// An inlay anchored past the line's last source byte appends at the
    /// end (no source tail to split).
    #[test]
    fn inlay_past_end_of_line_appends() {
        let theme = InlineDecorationsTheme::default();
        let spans = vec![src("foo")];
        let h = hint(0, 10, " : trailing", InlayHintKind::Other);
        let inlays = vec![(10usize, &h)];

        let out = splice_inlays_into_spans(&spans, &inlays, &theme);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "foo");
        assert!(!out[0].is_virtual);
        assert_eq!(out[1].text, " : trailing");
        assert!(out[1].is_virtual);
    }

    /// No inlays on the line returns the source spans unchanged.
    #[test]
    fn empty_inlays_returns_source_unchanged() {
        let theme = InlineDecorationsTheme::default();
        let spans = vec![src("hello"), src(" world")];
        let inlays: Vec<(usize, &InlayHintData)> = vec![];

        let out = splice_inlays_into_spans(&spans, &inlays, &theme);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "hello");
        assert_eq!(out[1].text, " world");
        assert!(out.iter().all(|s| !s.is_virtual));
    }

    /// Two inlays at the same byte offset stack in the order they appear
    /// in the input — e.g. a parameter hint followed by a type hint at
    /// the same source position should appear back-to-back.
    #[test]
    fn coincident_inlays_stack_in_input_order() {
        let theme = InlineDecorationsTheme::default();
        let spans = vec![src("xy")];
        let h1 = hint(0, 1, "[A]", InlayHintKind::Type);
        let h2 = hint(0, 1, "[B]", InlayHintKind::Parameter);
        let inlays = vec![(1usize, &h1), (1usize, &h2)];

        let out = splice_inlays_into_spans(&spans, &inlays, &theme);

        let texts: Vec<&str> = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["x", "[A]", "[B]", "y"]);
    }
}
