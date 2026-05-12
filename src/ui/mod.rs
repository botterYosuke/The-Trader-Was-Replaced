pub mod components;
pub mod systems;
pub mod window;
pub mod button;

use bevy::prelude::*;
use crate::ui::systems::{update_price_display, button_system, update_status_indicator};
use crate::ui::window::setup_ui;
use crate::ui::components::WindowManager;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WindowManager>()
            .add_systems(Startup, setup_ui)
            .add_systems(Update, (
                update_price_display,
                button_system,
                update_status_indicator,
            ));
    }
}
