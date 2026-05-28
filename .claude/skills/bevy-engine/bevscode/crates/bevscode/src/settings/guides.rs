//! Indent / bracket-pair guide settings — Monaco `editor.guides.*`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Guides {
    pub indentation: bool,
    pub highlight_active_indentation: bool,
    pub bracket_pairs: BracketPairsGuide,
    pub bracket_pairs_horizontal: BracketPairsGuide,
    pub highlight_active_bracket_pair: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum BracketPairsGuide {
    #[default]
    Off,
    Active,
    All,
}

impl Default for Guides {
    fn default() -> Self {
        Self {
            indentation: true,
            highlight_active_indentation: true,
            bracket_pairs: BracketPairsGuide::Off,
            bracket_pairs_horizontal: BracketPairsGuide::Active,
            highlight_active_bracket_pair: true,
        }
    }
}
