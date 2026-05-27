//! Shared helpers for the gutter-decoration sync systems.
//!
//! Each per-line decoration system (icons, bars, chevrons) needs the
//! same pool/diff/hide cycle over a `Vec<Entity>` of child UI Nodes
//! plus the same "buffer line → top-px" math. Centralising both keeps
//! each sync system focused on the parts that actually differ — which
//! lines to draw and which Components to spawn — instead of repeating
//! the per-frame pooling bookkeeping in three places.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::LineHeight;
use bevy_instanced_text::DisplayLayout;

use crate::settings::Padding;

/// Resolved per-row vertical placement in the gutter container's
/// logical px space.
pub(crate) struct RowGeometry {
    pub top_px: f32,
    pub line_height_px: f32,
}

impl RowGeometry {
    /// Compute the top-edge px for `buffer_line` by **reading the
    /// renderer's own `DisplayLayout`** — the same shaped-line array
    /// the code text view paints from. This naturally accounts for
    /// folding, soft-wrap, and any per-row `line_height` override the
    /// layout producer applied. Returns `None` when `buffer_line` does
    /// not appear in the layout (off-screen / hidden by a fold / layout
    /// hasn't run yet).
    ///
    /// The returned `top_px` is **relative to the
    /// [`GutterContainer`](crate::types::GutterContainer)'s content
    /// box**, which `sync_gutter_container` aligns to the code area's
    /// text row 0 via `Node::top = Padding::top`. So we strip the
    /// renderer's `text_area_top` (= `padding.top`) — and the scroll —
    /// from `y_top` here, since both will be re-applied by the
    /// container's own `top` and Bevy's scroll mechanism.
    pub(crate) fn compute(
        buffer_line: usize,
        font: &TextFont,
        line_height: &LineHeight,
        padding: &Padding,
        layout: &DisplayLayout,
    ) -> Option<Self> {
        let default_line_height =
            bevy_instanced_text::resolve_line_height(*line_height, font.font_size);
        if default_line_height <= 0.0 {
            return None;
        }
        let shaped = layout
            .lines
            .iter()
            .find(|l| l.buffer_row as usize == buffer_line)?;
        let line_height_px = shaped.line_height.unwrap_or(default_line_height);
        // `y_top` in the renderer's frame is `text_area_top - scroll +
        // display_row * line_height`. We want a value in
        // GutterContainer-local coords with scroll re-applied later by
        // the gutter's ScrollPosition mirror; subtract `padding.top`
        // here to undo the renderer's own padding offset.
        let top_px = (shaped.y_top - padding.top).round();
        Some(Self {
            top_px,
            line_height_px,
        })
    }
}

/// Group existing child entities by their owning editor so each
/// per-editor sync pass can index into its own slice. Returns a map
/// keyed by the editor Entity; the per-editor `Vec<Entity>` is the
/// pool the caller reuses (re-position + tint) or appends to (spawn).
///
/// `marker_editor` projects the marker Component back to its owning
/// editor (e.g. `|gi: &GutterIcon| gi.editor`).
pub(crate) fn group_pools_by_editor<'a, M, I, F>(
    items: I,
    marker_editor: F,
) -> HashMap<Entity, Vec<Entity>>
where
    I: Iterator<Item = (Entity, &'a M)>,
    M: 'a,
    F: Fn(&M) -> Entity,
{
    let mut by_editor: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (id, marker) in items {
        by_editor.entry(marker_editor(marker)).or_default().push(id);
    }
    by_editor
}

/// Inverse of `RowGeometry::compute`: given an editor-local y in
/// screen pixels (the same coordinate space as a UI pointer's
/// `local_pos.y`), return the buffer line the renderer painted at
/// that y. Returns `None` when the y falls outside any shaped row
/// (off-screen / before-first-row / after-last-row / layout not yet
/// produced).
///
/// This is what every click / hover hit-test should use instead of
/// `(y / line_height) as usize` + `FoldState::display_to_actual_line`.
/// The arithmetic-only path ignores soft-wrap continuation rows; the
/// renderer's `DisplayLayout` is the only place that knows where each
/// buffer line landed after fold + wrap + per-row line-height
/// overrides.
///
/// `local_y` should be in the editor's own logical px coordinate
/// frame — what `mouse::hit_to_local_px` returns. `ShapedLine::y_top`
/// is already in that frame (it's `text_area_top - scroll +
/// display_row * line_height`), so no caller-side padding / scroll
/// adjustment is needed.
pub fn buffer_line_at_y(
    layout: &bevy_instanced_text::DisplayLayout,
    local_y: f32,
) -> Option<usize> {
    for line in layout.lines.iter() {
        let row_h = line.line_height.unwrap_or(layout.line_height);
        if local_y < line.y_top {
            return None;
        }
        if local_y < line.y_top + row_h {
            return Some(line.buffer_row as usize);
        }
    }
    None
}

/// Diff-write a position-and-size update onto a Node, only touching
/// fields that actually changed. Pooled gutter children are stable
/// across frames; most updates only move on scroll, so this keeps
/// change-detection traffic low.
pub(crate) fn diff_place(node: &mut Node, left: f32, top: f32, width: f32, height: f32) {
    let target_left = Val::Px(left);
    let target_top = Val::Px(top);
    let target_w = Val::Px(width);
    let target_h = Val::Px(height);
    if node.left != target_left {
        node.left = target_left;
    }
    if node.top != target_top {
        node.top = target_top;
    }
    if node.width != target_w {
        node.width = target_w;
    }
    if node.height != target_h {
        node.height = target_h;
    }
}
