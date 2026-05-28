use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;

use crate::pipeline::parse_dirty;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseSet;

#[derive(Default)]
pub struct TreeSitterPlugin;

impl Plugin for TreeSitterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, parse_dirty.in_set(ParseSet));
    }
}
