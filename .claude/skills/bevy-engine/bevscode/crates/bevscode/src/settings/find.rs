//! Find/search widget — Monaco `editor.find`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Find {
    pub cursor_move_on_type: bool,
    pub seed_search_string_from_selection: SeedSearch,
    pub auto_find_in_selection: AutoFindInSelection,
    pub add_extra_space_on_top: bool,
    pub wrap: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum SeedSearch {
    Never,
    #[default]
    Always,
    Selection,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AutoFindInSelection {
    #[default]
    Never,
    Always,
    Multiline,
}

impl Default for Find {
    fn default() -> Self {
        Self {
            cursor_move_on_type: true,
            seed_search_string_from_selection: SeedSearch::Always,
            auto_find_in_selection: AutoFindInSelection::Never,
            add_extra_space_on_top: true,
            wrap: true,
        }
    }
}
