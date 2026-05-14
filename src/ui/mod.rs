pub mod button;
pub mod buying_power;
pub mod chart;
pub mod components;
pub mod floating_window;
pub mod footer;
pub mod menu_bar;
pub mod orders;
pub mod positions;
pub mod run_result_panel;
pub mod scenario_parser;
pub mod sidebar;
pub mod strategy_editor;
pub mod systems;
pub mod window;

use crate::ui::buying_power::buying_power_panel_system;
use crate::ui::chart::chart_render_system;
use crate::ui::components::{
    OpenStrategyRequested, PanelSpawnRequested, ScenarioMetadata, StrategyBuffer,
    StrategyRunRequested, WindowManager,
};
use crate::ui::floating_window::panel_spawn_dispatcher_system;
use crate::ui::footer::{
    spawn_footer, speed_button_system, transport_button_system, update_footer_system,
    update_speed_buttons_system,
};
use crate::ui::menu_bar::{
    handle_strategy_run_system, log_open_strategy_requested_system,
    log_strategy_run_requested_system, menu_button_system, open_strategy_buffer_system,
    spawn_menu_bar, update_strategy_status_label_system,
};
use crate::ui::orders::orders_panel_system;
use crate::ui::positions::positions_panel_system;
use crate::ui::run_result_panel::run_result_panel_system;
use crate::ui::scenario_parser::parse_scenario_system;
use crate::ui::sidebar::{panel_button_system, spawn_sidebar, update_sidebar_system};
use crate::ui::strategy_editor::strategy_editor_window_system;
use crate::ui::systems::{button_system, update_price_display, update_status_indicator};
use bevy::prelude::*;
use bevy_cosmic_edit::{CosmicEditPlugin, CosmicFontConfig, prelude::change_active_editor_sprite};
use bevy_egui::EguiPlugin;
use bevy_vector_shapes::Shape2dPlugin;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            Shape2dPlugin::default(),
            EguiPlugin,
            CosmicEditPlugin {
                font_config: CosmicFontConfig::default(),
            },
        ))
        .init_resource::<WindowManager>()
        .init_resource::<StrategyBuffer>()
        .add_event::<OpenStrategyRequested>()
        .add_event::<StrategyRunRequested>()
        .add_event::<PanelSpawnRequested>()
        .init_resource::<ScenarioMetadata>()
        .add_systems(Startup, (spawn_footer, spawn_menu_bar, spawn_sidebar))
        .add_systems(
            Update,
            (
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
                update_sidebar_system,
                panel_button_system,
                panel_spawn_dispatcher_system,
            ),
        )
        .add_systems(
            Update,
            (
                buying_power_panel_system,
                positions_panel_system,
                orders_panel_system,
            ),
        )
        .add_systems(Update, change_active_editor_sprite);
    }
}
