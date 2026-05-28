//! Editor-side cursor-line settings — Monaco `renderLineHighlight`,
//! `renderLineHighlightOnlyWhenFocus`, and the word-highlight band.
//!
//! Caret shape/blink (`CursorSettings`, `CursorStyle`) lives in
//! `bevy_instanced_text_interaction` and is re-exported below.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub use bevy_instanced_text_editor::{
    CursorBlinkingMode, CursorSettings, CursorStyle, SmoothCaretAnimation, SurroundingLinesStyle,
};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct CursorLine {
    pub render_line_highlight: RenderLineHighlight,
    pub only_when_focus: bool,
    pub border_width: f32,
    pub border_thickness: f32,
    pub border_alpha_multiplier: f32,
    pub border_color: Color,
    pub highlight_word: bool,
    pub word_highlight_color: Color,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum RenderLineHighlight {
    None,
    Gutter,
    #[default]
    Line,
    All,
}

impl Default for CursorLine {
    fn default() -> Self {
        Self {
            render_line_highlight: RenderLineHighlight::Line,
            only_when_focus: false,
            border_width: 1.0,
            border_thickness: 1.0,
            border_alpha_multiplier: 1.0,
            border_color: Color::srgba(0.4, 0.4, 0.4, 0.3),
            highlight_word: true,
            word_highlight_color: Color::srgba(0.4, 0.4, 0.4, 0.2),
        }
    }
}
