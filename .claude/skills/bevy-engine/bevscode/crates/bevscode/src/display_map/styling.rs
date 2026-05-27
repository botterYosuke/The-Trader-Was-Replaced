//! Helpers shared by the editor's syntax-styling producer system.
//!
//! Converts the editor's internal `LineSegment` shape into the engine's
//! `FormattedSpan` payload. The producer system `produce_line_styles` calls
//! `EditorSyntaxState::highlight_range` to get segments, then runs each
//! per-line slice through `segs_to_runs` before stuffing it into a
//! [`bevy_instanced_text::LineStyles`] map.

use bevy_instanced_text::{FormattedSpan, TextFormat};

use crate::types::LineSegment;

/// Convert a slice of `LineSegment`s (the editor's per-line styling shape)
/// into the engine's `FormattedSpan` payloads.
///
/// The engine overwrites `byte_range` after concatenation; we leave it
/// `0..0`. Empty-text segments are dropped because the engine indexes by
/// byte length and zero-len entries would shift later run boundaries
/// without effect.
pub(crate) fn segs_to_runs(segs: &[LineSegment]) -> Vec<FormattedSpan> {
    segs.iter()
        .filter(|s| !s.text.is_empty())
        .map(|s| {
            let mut format = TextFormat::fg(0..0, s.color)
                .with_scale(s.font_scale)
                .with_skew(s.skew)
                .with_corner_radius(s.corner_radius);
            if let Some(bg) = s.background {
                format = format.with_bg(bg);
            }
            FormattedSpan {
                text: s.text.clone(),
                format,
                is_virtual: false,
            }
        })
        .collect()
}
