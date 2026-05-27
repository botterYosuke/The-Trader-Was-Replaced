#![allow(clippy::type_complexity)]

//! Component-driven tree-sitter integration for Bevy.
//!
//! Parses text into syntax trees on a background thread and exposes the
//! results as ECS components. Rendering-agnostic — produces structured data
//! ([`SyntaxTree`], [`HighlightRange`]); hosts decide what to do with it.

pub mod highlight;
pub mod language;
pub mod pipeline;
pub mod plugin;
pub mod prelude;
pub mod tree_sitter;

pub use ::arborium::tree_sitter as ts;
pub use ::arborium;

pub use crate::highlight::{highlight_ranges, HighlightRange};
pub use crate::language::TreeSitterGrammar;
pub use crate::pipeline::{byte_to_point, ParseSource, ParseSourceComp, SyntaxTree};
pub use crate::plugin::{ParseSet, TreeSitterPlugin};
pub use crate::tree_sitter::TreeSitterProvider;
