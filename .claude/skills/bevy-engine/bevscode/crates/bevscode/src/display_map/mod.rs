//! Display map — wires the editor's fold / syntax / wrap state into the
//! engine's per-frame layout system via plain-data Components.
//!
//! The wrap-aware layout walker lives engine-side in
//! `bevy_instanced_text::view::layout_builder`, driven by `produce_layouts`.
//! This module's [`DisplayMapPlugin`] inserts the engine's
//! `bevy_instanced_text::HiddenLines` / `bevy_instanced_text::LineStyles` /
//! `bevy_instanced_text::LayoutWrap` Components on each `CodeEditor` entity
//! and refreshes them from `FoldState`, the per-entity `EditorSyntaxState`,
//! `Wrapping`, etc. via producer systems in [`plugin::LayoutSyncSet`].
//! The engine's `produce_layouts` reads those Components on each layout pass.
//!
//! Cursor/selection systems map buffer positions to display rows via
//! `DisplayLayout::buffer_to_display`, which scans the visible window's
//! rows directly — there is no separate transform-stack abstraction.

pub mod plugin;
pub mod styling;

#[cfg(test)]
mod plugin_tests;

pub use plugin::{DisplayMapPlugin, LayoutSyncSet};
