//! Minimap settings — Monaco `editor.minimap`.
//!
//! No minimap renderer ships in bevscode yet; this Component exists for API
//! parity. `enabled` defaults to `false` (Monaco defaults to `true`).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Minimap {
    pub enabled: bool,
    pub autohide: bool,
    pub side: MinimapSide,
    pub size: MinimapSize,
    pub show_slider: ShowSlider,
    pub render_characters: bool,
    pub max_column: u32,
    pub scale: u32,
    pub show_region_section_headers: bool,
    pub show_mark_section_headers: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum MinimapSide {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum MinimapSize {
    #[default]
    Proportional,
    Fill,
    Fit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum ShowSlider {
    Always,
    #[default]
    Mouseover,
}

impl Default for Minimap {
    fn default() -> Self {
        Self {
            enabled: false,
            autohide: false,
            side: MinimapSide::Right,
            size: MinimapSize::Proportional,
            show_slider: ShowSlider::Mouseover,
            render_characters: true,
            max_column: 120,
            scale: 1,
            show_region_section_headers: true,
            show_mark_section_headers: true,
        }
    }
}
