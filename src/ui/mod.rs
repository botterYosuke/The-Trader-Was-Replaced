pub mod components;
pub mod systems;
pub mod window;
pub mod button;
pub mod chart;
pub mod footer;
pub mod menu_bar;

use bevy::prelude::*;
use crate::ui::footer::{spawn_footer, transport_button_system, update_footer_system};
use crate::ui::menu_bar::{
    log_open_strategy_requested_system,
    menu_button_system,
    open_strategy_buffer_system,
    spawn_menu_bar,
    update_strategy_status_label_system,
};
use crate::ui::systems::{update_price_display, button_system, update_status_indicator};
use crate::ui::window::setup_ui;
use crate::ui::components::{OpenStrategyRequested, StrategyBuffer, WindowManager};
use crate::ui::chart::chart_render_system;
use bevy_vector_shapes::Shape2dPlugin;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Shape2dPlugin::default())
            .init_resource::<WindowManager>()
            .init_resource::<StrategyBuffer>()
            .add_event::<OpenStrategyRequested>()
            .add_systems(Startup, (setup_ui, spawn_footer, spawn_menu_bar))
            .add_systems(Update, (
                update_price_display,
                button_system,
                update_status_indicator,
                chart_render_system,
                update_footer_system,
                transport_button_system,
                menu_button_system,
                log_open_strategy_requested_system,
                open_strategy_buffer_system,
                update_strategy_status_label_system,
            ));
    }
}
