pub mod components;
pub mod systems;
pub mod window;
pub mod button;
pub mod chart;
pub mod footer;

use bevy::prelude::*;
use crate::ui::footer::{spawn_footer, update_footer_system};
use crate::ui::systems::{update_price_display, button_system, update_status_indicator};
use crate::ui::window::setup_ui;
use crate::ui::components::WindowManager;
use crate::ui::chart::chart_render_system;
use bevy_vector_shapes::Shape2dPlugin;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Shape2dPlugin::default())
            .init_resource::<WindowManager>()
            .add_systems(Startup, (setup_ui, spawn_footer))
            .add_systems(Update, (
                update_price_display,
                button_system,
                update_status_indicator,
                chart_render_system,
                update_footer_system,
            ));
    }
}
