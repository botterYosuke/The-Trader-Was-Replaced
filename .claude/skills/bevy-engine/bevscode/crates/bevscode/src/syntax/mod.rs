//! Editor-side syntax glue.
//!
//! The structural tree-sitter machinery (`TreeSitterProvider`, `highlight_ranges`,
//! `Language`, …) lives in [`bevy_tree_sitter`]. This module
//! supplies just the editor-side bridge: theme color mapping and (when the
//! `tree-sitter` feature is on) the cache + system that turns structural
//! `HighlightRange`s into colored `LineSegment`s.

pub mod highlighter;
pub mod language_swap;

pub use highlighter::map_highlight_color;
