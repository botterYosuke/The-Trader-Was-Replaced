//! `QueryData` groups for the most-co-queried Component bundles.
//!
//! Editor systems regularly need the same combinations of buffer/cursor
//! state, viewport metrics, and chrome settings. Pulling each as a flat
//! tuple makes systems noisy; these groups let callers write
//! `Query<(EditorBufferView, EditorLayoutView, ...), With<CodeEditor>>`
//! and dot into fields by name.

use bevy::ecs::query::QueryData;
use bevy::text::TextFont;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::{DisplayLayout, MonoCellWidth, TextBounds, TextBuffer};
use bevy_instanced_text_editor::{CursorState, RopeBuffer, ScrollConfig};

use crate::plugin::ScrollAnimator;
use crate::text_view::ContentMetrics;
use crate::types::FoldState;

/// Buffer + fold state. Every overlay producer that reads text and needs
/// to skip folded lines wants this group. CursorState / SelectionState
/// are taken as `&'static` next to it on the rare system that needs
/// them — most systems only need one or the other.
#[derive(QueryData)]
pub struct EditorBufferView {
    pub buffer: &'static TextBuffer<RopeBuffer>,
    pub fold: &'static FoldState,
}

/// Viewport + scroll + mono-cell metrics + optional display layout. The
/// shaped layout is `Option<&...>` because some systems (gutter setup,
/// scroll animator) run before it exists.
#[derive(QueryData)]
pub struct EditorLayoutView {
    pub computed: &'static ComputedNode,
    pub scroll: &'static ScrollPosition,
    pub mono: &'static MonoCellWidth,
    pub layout: Option<&'static DisplayLayout>,
}

/// Font + line-height — readers that need `resolve_line_height` /
/// font_size together (line numbers, indent guides, syntax sizing).
#[derive(QueryData)]
pub struct EditorFontView {
    pub font: &'static TextFont,
    pub line_height: &'static bevy::text::LineHeight,
}

/// Buffer + layout + font: the read-only triple most overlay producers
/// need. Replaces the `(EditorBufferView, EditorLayoutView,
/// EditorFontView)` tuple that was repeated across 6+ systems.
#[derive(QueryData)]
pub struct EditorRenderView {
    pub buffer: &'static TextBuffer<RopeBuffer>,
    pub fold: &'static FoldState,
    pub computed: &'static ComputedNode,
    pub scroll: &'static ScrollPosition,
    pub mono: &'static MonoCellWidth,
    pub layout: Option<&'static DisplayLayout>,
    pub font: &'static TextFont,
    pub line_height: &'static bevy::text::LineHeight,
}

/// Pre-computed viewport metrics derived from an [`EditorRenderViewItem`].
pub struct ViewportMetrics {
    pub inv_scale: f32,
    pub char_width: f32,
    pub line_height: f32,
    pub viewport_height: f32,
    pub viewport_width: f32,
    pub text_area_top: f32,
    pub text_area_left: f32,
}

impl EditorRenderViewItem<'_, '_> {
    pub fn metrics(&self) -> ViewportMetrics {
        let inv = self.computed.inverse_scale_factor();
        ViewportMetrics {
            inv_scale: inv,
            char_width: self.mono.px,
            line_height: bevy_instanced_text::resolve_line_height(
                *self.line_height,
                self.font.font_size,
            ),
            viewport_height: self.computed.size().y * inv,
            viewport_width: self.computed.size().x * inv,
            text_area_top: self.computed.content_inset().min_inset.y * inv,
            text_area_left: self.computed.content_inset().min_inset.x * inv,
        }
    }
}

/// Gutter-side layout: font + line-height + padding + display layout.
/// Used by gutter decoration sync systems (chevrons, markers, bars).
#[derive(QueryData)]
pub struct GutterLayoutView {
    pub font: &'static TextFont,
    pub line_height: &'static bevy::text::LineHeight,
    pub padding: &'static crate::settings::Padding,
    pub layout: &'static DisplayLayout,
    pub gutter: &'static crate::settings::GutterConfig,
}

/// Scroll-target shape — animator + content metrics + scroll config +
/// mutable cursor + optional [`TextBounds`] (for wrap-aware horizontal
/// gating). Mutability lets the auto-scroll system both *seed* the
/// animator's target and *clear* the cursor's `last_cursor_pos`.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct ScrollTargetView {
    pub animator: &'static mut ScrollAnimator,
    pub metrics: &'static ContentMetrics,
    pub cursor: &'static mut CursorState,
    pub scroll_cfg: &'static ScrollConfig,
    pub bounds: Option<&'static TextBounds>,
}
