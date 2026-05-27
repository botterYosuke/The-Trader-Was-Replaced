//! Per-entity syntax highlighting palette.
//!
//! Component on the `CodeEditor` entity (cascaded via `#[require]` when
//! `tree-sitter` is on). Capture name → color mapping lives in
//! `crate::syntax::map_highlight_color`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-entity syntax highlight color palette (tree-sitter capture → color).
///
/// Cascaded onto every `CodeEditor` entity via `#[require]` when the
/// `tree-sitter` feature is enabled. Override individual fields at spawn
/// time or mutate via `Query<&mut SyntaxColors, With<CodeEditor>>`.
#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct SyntaxColors {
    pub keyword: Color,
    pub function: Color,
    pub method: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub variable: Color,
    pub operator: Color,
    pub constant: Color,
    pub type_name: Color,
    pub parameter: Color,
    pub property: Color,
    pub punctuation: Color,
    pub label: Color,
    pub constructor: Color,
    pub escape: Color,
    pub embedded: Color,
}

impl Default for SyntaxColors {
    fn default() -> Self {
        Self {
            keyword: Color::srgb(0.337, 0.612, 0.839),
            function: Color::srgb(0.863, 0.863, 0.667),
            method: Color::srgb(0.863, 0.863, 0.667),
            string: Color::srgb(0.808, 0.569, 0.471),
            number: Color::srgb(0.710, 0.808, 0.659),
            comment: Color::srgb(0.416, 0.600, 0.333),
            variable: Color::srgb(0.612, 0.863, 0.996),
            operator: Color::srgb(0.831, 0.831, 0.831),
            constant: Color::srgb(0.310, 0.757, 1.0),
            type_name: Color::srgb(0.306, 0.788, 0.690),
            parameter: Color::srgb(0.612, 0.863, 0.996),
            property: Color::srgb(0.612, 0.863, 0.996),
            punctuation: Color::srgb(0.831, 0.831, 0.831),
            label: Color::srgb(0.337, 0.612, 0.839),
            constructor: Color::srgb(0.306, 0.788, 0.690),
            escape: Color::srgb(0.851, 0.486, 0.682),
            embedded: Color::srgb(0.831, 0.831, 0.831),
        }
    }
}
