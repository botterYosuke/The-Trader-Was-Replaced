pub mod components;
pub mod systems;
pub mod window;
pub mod button;
pub mod chart;
pub mod footer;
pub mod menu_bar;
pub mod run_result_panel;
pub mod scenario_parser;
pub mod sidebar;
pub mod strategy_editor;

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use crate::ui::footer::{spawn_footer, speed_button_system, transport_button_system, update_footer_system, update_speed_buttons_system};
use crate::ui::run_result_panel::run_result_panel_system;
use crate::ui::strategy_editor::strategy_editor_window_system;
use crate::ui::menu_bar::{
    handle_strategy_run_system,
    log_open_strategy_requested_system,
    log_strategy_run_requested_system,
    menu_button_system,
    open_strategy_buffer_system,
    spawn_menu_bar,
    update_strategy_status_label_system,
};
use crate::ui::systems::{update_price_display, button_system, update_status_indicator};
use crate::ui::window::setup_ui;
use crate::ui::components::{OpenStrategyRequested, ScenarioMetadata, StrategyBuffer, StrategyRunRequested, WindowManager};
use crate::ui::chart::chart_render_system;
use crate::ui::scenario_parser::parse_scenario_system;
use crate::ui::sidebar::sidebar_system;
use bevy_vector_shapes::Shape2dPlugin;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((Shape2dPlugin::default(), EguiPlugin))
            .init_resource::<WindowManager>()
            .init_resource::<StrategyBuffer>()
            .add_event::<OpenStrategyRequested>()
            .add_event::<StrategyRunRequested>()
            .init_resource::<ScenarioMetadata>()
            .add_systems(Startup, (setup_ui, spawn_footer, spawn_menu_bar))
            .add_systems(Update, (
                update_price_display,
                button_system,
                update_status_indicator,
                chart_render_system,
                update_footer_system,
                transport_button_system,
                speed_button_system,
                update_speed_buttons_system,
                menu_button_system,
                log_open_strategy_requested_system,
                open_strategy_buffer_system,
                update_strategy_status_label_system,
                strategy_editor_window_system,
                run_result_panel_system,
                log_strategy_run_requested_system,
                handle_strategy_run_system,
                parse_scenario_system,
                sidebar_system,
            ));
    }
}
