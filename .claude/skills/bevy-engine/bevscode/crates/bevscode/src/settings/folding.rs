//! Folding settings — Monaco `editor.folding*`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Folding {
    pub enabled: bool,
    pub strategy: FoldingStrategy,
    pub highlight: bool,
    pub imports_by_default: bool,
    pub max_regions: u32,
    pub show_controls: ShowFoldingControls,
    pub unfold_on_click_after_eol: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum FoldingStrategy {
    #[default]
    Auto,
    Indentation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum ShowFoldingControls {
    Always,
    Never,
    #[default]
    Mouseover,
}

impl Default for Folding {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: FoldingStrategy::Auto,
            highlight: true,
            imports_by_default: false,
            max_regions: 5000,
            show_controls: ShowFoldingControls::Mouseover,
            unfold_on_click_after_eol: false,
        }
    }
}
