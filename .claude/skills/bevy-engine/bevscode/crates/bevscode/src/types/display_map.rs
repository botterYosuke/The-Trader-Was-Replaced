//! Styled text segment types used by the syntax highlighting layer.

use bevy::prelude::*;

/// A segment of text with a specific color on a specific line.
#[derive(Clone, Debug)]
pub struct LineSegment {
    pub text: String,
    pub color: Color,
    /// Optional background color rendered as a solid rectangle behind the text.
    pub background: Option<Color>,
    /// Corner radius for the background rectangle (0 = sharp corners).
    pub corner_radius: f32,
    /// Font size scale factor (1.0 = normal, 1.3 = header, etc.). 0.0 means use default.
    pub font_scale: f32,
    /// Horizontal skew for italic simulation (0.0 = normal, ~0.2 = italic).
    pub skew: f32,
}
