//! Inline LSP decorations: document highlights (engine overlay rects).
//!
//! Document highlights go through the engine's [`RectOverlay`] path so
//! they share the editor's draw call and stay pixel-aligned with the
//! glyph grid.
//!
//! Inlay hints used to render here as `bevy_ui` text overlays. They now
//! go through the engine's shape pipeline as virtual [`FormattedSpan`]s
//! spliced into [`LineStyles`] (see
//! [`crate::lsp_ui::inlay_splice::splice_inlays_into_line_styles`]) — that path
//! shapes them inline with source glyphs so following source text shifts
//! right by the hint's width, eliminating the overlap the overlay path
//! had. The [`InlineDecorationsTheme`] colors / scale factor live on
//! here because the splicer reads them as a `Resource`.
//!
//! [`FormattedSpan`]: bevy_instanced_text::FormattedSpan
//! [`LineStyles`]: bevy_instanced_text::LineStyles

use bevy::prelude::*;
use bevy_instanced_text::{
    CornerRadii, DisplayLayout, MonoCellWidth, RectOverlay, RowVertical, TextOverlays,
};

use crate::lsp_ui::components::DocumentHighlightData;
use crate::types::CodeEditor;

/// Per-editor styling for inline LSP decorations. Hosts override by
/// `app.insert_resource(InlineDecorationsTheme { .. })`.
#[derive(Resource, Clone, Debug)]
pub struct InlineDecorationsTheme {
    /// Foreground color for type-annotation inlay hints.
    pub inlay_type: Color,
    /// Foreground color for parameter-name inlay hints.
    pub inlay_parameter: Color,
    /// Foreground color for any other inlay-hint kind.
    pub inlay_other: Color,
    /// Multiplier on the editor's font size for inlay hint glyphs (passed
    /// through to `TextFormat.font_scale` on the virtual span).
    pub inlay_font_scale: f32,
    pub highlight_read: Color,
    pub highlight_write: Color,
}

impl Default for InlineDecorationsTheme {
    fn default() -> Self {
        Self {
            inlay_type: Color::srgba(0.5, 0.7, 0.9, 0.7),
            inlay_parameter: Color::srgba(0.7, 0.6, 0.9, 0.7),
            inlay_other: Color::srgba(0.6, 0.6, 0.6, 0.7),
            inlay_font_scale: 0.85,
            highlight_read: Color::srgba(0.5, 0.6, 0.8, 0.25),
            highlight_write: Color::srgba(0.8, 0.5, 0.3, 0.3),
        }
    }
}

/// Push a [`RectOverlay`] for every [`DocumentHighlightData`] into the
/// editor's [`TextOverlays`] (engine overlay slot `z = -2`, between
/// selections at `-1` and the line background at `0`).
pub fn render_document_highlights(
    highlights: Query<&DocumentHighlightData>,
    mut editors: Query<(&MonoCellWidth, &DisplayLayout, &mut TextOverlays), With<CodeEditor>>,
    theme: Res<InlineDecorationsTheme>,
) {
    let Ok((mono, layout, mut overlays)) = editors.single_mut() else {
        return;
    };

    overlays.0.retain(|r| r.z != -2);

    for highlight in highlights.iter() {
        let color = if highlight.is_write {
            theme.highlight_write
        } else {
            theme.highlight_read
        };

        let buffer_row = highlight.line;
        let start_byte = highlight.start_character as usize;
        let Some((display_row, start_byte_in_row)) =
            layout.buffer_to_display(buffer_row, start_byte)
        else {
            continue;
        };

        let start_x = layout
            .x_at_byte(display_row, start_byte_in_row)
            .unwrap_or(highlight.start_character as f32 * mono.px);

        let end_x = if highlight.end_character == u32::MAX {
            layout
                .lines
                .iter()
                .find(|l| l.display_row == display_row)
                .and_then(|l| layout.x_at_byte(display_row, l.text.len()))
                .unwrap_or_else(|| {
                    start_x
                        + highlight
                            .end_character
                            .saturating_sub(highlight.start_character)
                            .min(200) as f32
                            * mono.px
                })
        } else {
            // Stop at the source glyphs' trailing edge: a document
            // highlight on a token immediately followed by an inlay hint
            // (parameter / type) would otherwise include the inlay in
            // its rectangle.
            let end_byte = highlight.end_character as usize;
            let end_byte_in_row = end_byte.saturating_sub(start_byte) + start_byte_in_row;
            layout
                .x_after_source_range(display_row, start_byte_in_row, end_byte_in_row)
                .unwrap_or(
                    start_x
                        + (highlight.end_character - highlight.start_character) as f32 * mono.px,
                )
        };

        if end_x <= start_x {
            continue;
        }

        overlays.0.push(RectOverlay {
            display_row,
            x_range: start_x..end_x,
            vertical: RowVertical::Full,
            color,
            z: -2,
            corners: CornerRadii::ZERO,
        });
    }
}
