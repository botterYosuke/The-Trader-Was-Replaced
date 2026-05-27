//! Sticky scroll — Monaco `editor.stickyScroll`.
//!
//! No renderer yet; `enabled` defaults to `false` (Monaco defaults to `true`).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct StickyScroll {
    pub enabled: bool,
    pub max_line_count: u32,
    pub scroll_with_editor: bool,
}

impl Default for StickyScroll {
    fn default() -> Self {
        Self {
            enabled: false,
            max_line_count: 5,
            scroll_with_editor: true,
        }
    }
}
