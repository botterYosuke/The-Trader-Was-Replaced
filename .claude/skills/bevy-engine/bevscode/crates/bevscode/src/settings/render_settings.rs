//! Rendering toggles — Monaco `renderWhitespace`, `renderControlCharacters`,
//! `renderFinalNewline`, `renderValidationDecorations`, `stopRenderingLineAfter`,
//! and the `colorDecorators*` family.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct RenderSettings {
    pub render_whitespace: RenderWhitespace,
    pub render_control_characters: bool,
    pub render_final_newline: RenderFinalNewline,
    #[cfg(feature = "lsp")]
    pub render_validation_decorations: RenderValidationDecorations,
    pub stop_rendering_line_after: u32,
    pub color_decorators: bool,
    pub color_decorators_activated_on: ColorDecoratorsActivatedOn,
    pub color_decorators_limit: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum RenderWhitespace {
    None,
    Boundary,
    #[default]
    Selection,
    Trailing,
    All,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum RenderFinalNewline {
    #[default]
    On,
    Off,
    Dimmed,
}

#[cfg(feature = "lsp")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum RenderValidationDecorations {
    #[default]
    Editable,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum ColorDecoratorsActivatedOn {
    #[default]
    ClickAndHover,
    Click,
    Hover,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            render_whitespace: RenderWhitespace::Selection,
            render_control_characters: true,
            render_final_newline: RenderFinalNewline::On,
            #[cfg(feature = "lsp")]
            render_validation_decorations: RenderValidationDecorations::Editable,
            stop_rendering_line_after: 10_000,
            color_decorators: true,
            color_decorators_activated_on: ColorDecoratorsActivatedOn::ClickAndHover,
            color_decorators_limit: 500,
        }
    }
}
