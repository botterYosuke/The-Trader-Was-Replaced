//! Editor-side text-view module.
//!
//! The generic primitives (`TextView`, `DisplayLayout`, `ShapedLine`,
//! `TextFormat`, `RectOverlay`, `TextBuffer<RopeBuffer>`, `ScrollState`, `ContentMetrics`,
//! `render_layout`, `InstancedTextPlugin`,
//! `InstancedTextPlugins`, …) live in [`bevy_instanced_text`] and are re-exported
//! here so existing `use bevy_code_editor::text_view::…;` paths keep resolving.
//!
//! Interaction (`TextViewDragState`, `ScrollConfig`,
//! `InstancedTextInteractionPlugin`, `screen_to_char_pos`, `copy_selection`)
//! lives in [`bevy_instanced_text_editor`] and is re-exported here so the same
//! `use bevy_code_editor::text_view::…;` paths continue to resolve.
//!
//! Selection state for editor entities lives on `SelectionState` (also in
//! `bevy_instanced_text_editor`). Pre-Phase 30A there was a parallel
//! `TextViewSelectionState` Component for picking-driven selection; both
//! stores have been collapsed into the unified `SelectionState`.

pub use bevy_instanced_text::view::{
    glyph, overlay, pipeline, render, text, ContentMetrics, DisplayLayout, GlyphBatchComponent,
    RectOverlay, RowVertical, ShapedLine, TextBuffer, TextFormat, TextOverlays, TextUnderlays,
    TextViewBatchEntity, TextViewRenderSet,
};

pub use bevy_instanced_text_editor::{
    copy_selection, screen_to_char_pos, InstancedTextInteractionPlugin, RopeBuffer, ScrollConfig,
    TextViewDragState,
};
