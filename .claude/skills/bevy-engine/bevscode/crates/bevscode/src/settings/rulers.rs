//! Vertical column rulers — Monaco `editor.rulers`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Default, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Rulers(pub Vec<RulerOption>);

#[derive(Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct RulerOption {
    pub column: u32,
    pub color: Option<Color>,
}
