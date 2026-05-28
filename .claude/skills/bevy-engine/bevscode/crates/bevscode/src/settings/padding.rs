//! Editor content padding — Monaco `editor.padding`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Padding {
    pub top: f32,
    pub bottom: f32,
}

impl Default for Padding {
    fn default() -> Self {
        Self {
            top: 10.0,
            bottom: 0.0,
        }
    }
}
