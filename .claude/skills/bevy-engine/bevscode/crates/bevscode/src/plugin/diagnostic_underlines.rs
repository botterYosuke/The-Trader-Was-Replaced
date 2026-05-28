//! Wavy / squiggle underlines for LSP diagnostics.
//!
//! Consumes `DiagnosticMarker` entities spawned by `on_lsp_diagnostics` and
//! produces a per-editor [`DiagnosticUnderlineRects`] Vec which the central
//! `merge_overlay_components` system folds into `TextOverlays`. Tinted by
//! [`DiagnosticColors`].
//!
//! The wave is synthesised as a string of short rounded-pill `RectOverlay`s
//! using `RowVertical::UnderBaseline`, with per-tooth amplitude stepped
//! from a cosine envelope and SDF corner radii on each pill. Tooth pitch
//! and thickness scale with the inverse of the DPI factor so the wave
//! stays the same logical size across 1×/2× displays.

use bevy::prelude::*;
use bevy_instanced_text::{
    visible_buffer_range, CornerRadii, RectOverlay, RowVertical, TextBounds,
};
use lsp_types::DiagnosticSeverity;

use crate::lsp_ui::systems::DiagnosticMarker;
use crate::settings::{
    DiagnosticColors, EditorRenderView, RenderSettings, RenderValidationDecorations,
};
use crate::types::CodeEditor;
use crate::ui_kit::DiagnosticTokens;

/// Wavy underline overlays per visible diagnostic — written by
/// `update_diagnostic_underlines`, merged into `TextOverlays` by
/// `merge_overlay_components`.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct DiagnosticUnderlineRects(pub Vec<RectOverlay>);

/// Logical-px width of each pill in the wave. The system passes the
/// inverse DPI factor so the on-screen width stays stable on hi-DPI.
const SQUIGGLE_TOOTH_PX: f32 = 1.5;
/// Baseline gap below the text baseline (lowest point of the wave).
const SQUIGGLE_BASE_GAP: f32 = 1.0;
/// Peak additional gap above [`SQUIGGLE_BASE_GAP`] at the crest.
const SQUIGGLE_AMPLITUDE_PX: f32 = 1.25;
/// Pills per full sinusoidal period (down → up → down).
const SQUIGGLE_TEETH_PER_PERIOD: usize = 6;

pub(crate) fn update_diagnostic_underlines(
    diagnostics: Query<&DiagnosticMarker>,
    tokens: Option<Res<DiagnosticTokens>>,
    mut editors: Query<
        (
            EditorRenderView,
            Option<&TextBounds>,
            &DiagnosticColors,
            &RenderSettings,
            &crate::settings::Misc,
            &mut DiagnosticUnderlineRects,
        ),
        With<CodeEditor>,
    >,
) {
    let tokens = tokens.map(|t| t.clone()).unwrap_or_default();
    for (rv, bounds, colors, render, misc, mut out_rects) in editors.iter_mut() {
        let render_decorations = match render.render_validation_decorations {
            RenderValidationDecorations::Off => false,
            RenderValidationDecorations::On => true,
            RenderValidationDecorations::Editable => !misc.read_only,
        };
        if !render_decorations {
            if !out_rects.0.is_empty() {
                out_rects.0.clear();
            }
            continue;
        }

        let Some(layout) = rv.layout else {
            continue;
        };

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
            None,
        );

        let mut new_rects: Vec<RectOverlay> = Vec::new();

        for d in diagnostics.iter() {
            let buffer_line = d.line;
            if buffer_line < visible.start || buffer_line >= visible.end {
                continue;
            }
            if rv.fold.is_line_hidden(buffer_line) {
                continue;
            }
            let rope = rv.buffer.rope();
            if buffer_line >= rope.len_lines() {
                continue;
            }
            let line = rope.line(buffer_line);
            let line_chars = line.len_chars().saturating_sub(1).max(1);

            let raw_start = d.range.start.character as usize;
            let raw_end = if d.range.end.line == d.range.start.line {
                d.range.end.character as usize
            } else {
                line_chars + 1
            };

            // A zero-width LSP range (start == end, e.g. rust-analyzer's
            // `Syntax Error: expected an item` at col 0..0) would yield a
            // ~one-char squiggle that's easy to miss. Widen it to span the
            // line's non-whitespace content so the user actually sees the
            // marker. Matches VSCode's behavior for zero-width diagnostics.
            let (start_char, end_char) = if raw_end <= raw_start {
                let first_non_ws = line.chars().position(|c| !c.is_whitespace()).unwrap_or(0);
                (first_non_ws.min(line_chars), line_chars + 1)
            } else {
                (raw_start.min(line_chars), raw_end.min(line_chars + 1))
            };

            let s_byte = line.slice(..start_char.min(line.len_chars())).len_bytes();
            let e_byte = line.slice(..end_char.min(line.len_chars())).len_bytes();

            let (start_row, start_byte_in_row) = layout
                .buffer_to_display(buffer_line as u32, s_byte)
                .unwrap_or((rv.fold.actual_to_display_line(buffer_line) as u32, s_byte));
            let (end_row, end_byte_in_row) = layout
                .buffer_to_display(buffer_line as u32, e_byte)
                .unwrap_or((rv.fold.actual_to_display_line(buffer_line) as u32, e_byte));
            let start_x = layout
                .x_at_byte(start_row, start_byte_in_row)
                .unwrap_or(start_char as f32 * m.char_width);
            // Single-row squiggles end at the source glyph's trailing
            // edge — `x_at_byte(end)` would jump past an inlay anchored
            // at `end` and paint the squiggle under the inlay too.
            let end_x = if start_row == end_row {
                layout
                    .x_after_source_range(end_row, start_byte_in_row, end_byte_in_row)
                    .unwrap_or(end_char as f32 * m.char_width)
            } else {
                layout
                    .x_at_byte(end_row, end_byte_in_row)
                    .unwrap_or(end_char as f32 * m.char_width)
            };

            let color = color_for(d.severity, colors);

            if start_row == end_row {
                push_squiggle(
                    &mut new_rects,
                    start_row,
                    start_x..end_x,
                    color,
                    m.inv_scale,
                    &tokens,
                );
            } else {
                let start_row_end = layout
                    .lines
                    .iter()
                    .find(|l| l.display_row == start_row)
                    .and_then(|l| layout.x_at_byte(start_row, l.text.len()))
                    .unwrap_or(end_char as f32 * m.char_width);
                push_squiggle(
                    &mut new_rects,
                    start_row,
                    start_x..start_row_end,
                    color,
                    m.inv_scale,
                    &tokens,
                );
                for r in (start_row + 1)..end_row {
                    push_squiggle(&mut new_rects, r, 0.0..start_row_end, color, m.inv_scale, &tokens);
                }
                push_squiggle(&mut new_rects, end_row, 0.0..end_x, color, m.inv_scale, &tokens);
            }
        }

        if out_rects.0 != new_rects {
            if !new_rects.is_empty() {
                bevy::log::info!(
                    "[LSP] squiggles: emitted {} rects from {} diagnostics (visible rows {}..{})",
                    new_rects.len(),
                    diagnostics.iter().count(),
                    visible.start,
                    visible.end,
                );
            }
            out_rects.0 = new_rects;
        }
    }
}

fn color_for(severity: DiagnosticSeverity, colors: &DiagnosticColors) -> Color {
    match severity {
        DiagnosticSeverity::ERROR => colors.error,
        DiagnosticSeverity::WARNING => colors.warning,
        DiagnosticSeverity::INFORMATION => colors.info,
        _ => colors.hint,
    }
}

/// Emit a wave under the given x range as rounded-pill `UnderBaseline`
/// rects whose vertical gap follows a cosine envelope across
/// [`SQUIGGLE_TEETH_PER_PERIOD`] teeth. `inv` is the host's
/// `ComputedNode::inverse_scale_factor()` — multiplied through the
/// pitch and thickness so on-screen size stays stable on hi-DPI.
fn push_squiggle(
    out: &mut Vec<RectOverlay>,
    display_row: u32,
    x_range: std::ops::Range<f32>,
    color: Color,
    inv: f32,
    tokens: &DiagnosticTokens,
) {
    if x_range.end <= x_range.start {
        return;
    }
    let dpi = inv.max(1.0);
    let tooth_px = SQUIGGLE_TOOTH_PX * dpi;
    let thickness = tokens.squiggle_thickness * dpi;
    let radius = (thickness * 0.5).min(tooth_px * 0.5);
    let tint = color.with_alpha(color.alpha() * tokens.squiggle_alpha);
    let teeth = SQUIGGLE_TEETH_PER_PERIOD.max(2) as f32;

    let mut x = x_range.start;
    let mut step: usize = 0;
    while x < x_range.end {
        let seg_end = (x + tooth_px).min(x_range.end);
        let t = (step as f32 % teeth) / teeth;
        let gap = SQUIGGLE_BASE_GAP
            + SQUIGGLE_AMPLITUDE_PX * 0.5 * (1.0 - (t * std::f32::consts::TAU).cos());
        out.push(RectOverlay {
            display_row,
            x_range: x..seg_end,
            vertical: RowVertical::UnderBaseline { thickness, gap },
            color: tint,
            z: 0,
            corners: CornerRadii::uniform(radius),
        });
        x = seg_end;
        step += 1;
    }
}
