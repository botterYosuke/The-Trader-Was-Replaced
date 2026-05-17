pub mod button;
pub mod buying_power;
pub mod chart;
pub mod components;
pub mod editor_history;
pub mod floating_window;
pub mod footer;
pub mod layout_persistence;
pub mod menu_bar;
pub mod orders;
pub mod positions;
pub mod run_result_panel;
pub mod scenario_parser;
pub mod sidebar;
pub mod strategy_editor;
pub mod systems;
pub mod window;

pub use components::{
    ChartInstrument, InstrumentRegistry, ScenarioFileWatchState, ScenarioInstrumentsWritebackState,
    ScenarioLoadedFromFile, ScenarioWritebackPaths,
};

use crate::ui::buying_power::buying_power_panel_system;
use crate::ui::chart::chart_render_system;
use crate::ui::components::{
    OpenMenu, PanelSpawnRequested, PendingStrategyFragments, RedoMenuRequested, RegionKeyAllocator,
    ScenarioMetadata, StrategyBuffer, StrategyFileLoadRequested, StrategyRunRequested,
    UndoMenuRequested, WindowManager,
};
use crate::ui::editor_history::{
    ActiveDrag, AppHistory, PendingStrategySnapshotRestore, UndoRedoApplied,
};
use crate::ui::floating_window::panel_spawn_dispatcher_system;
use crate::ui::footer::{
    footer_pause_resume_system, spawn_footer, speed_button_system, transport_button_system,
    update_footer_system, update_speed_buttons_system,
};
use crate::ui::menu_bar::{
    handle_strategy_file_load_system, handle_strategy_run_system,
    log_strategy_file_load_requested_system, log_strategy_run_requested_system, menu_item_system,
    menu_keyboard_system, menu_top_level_system, restore_last_strategy_system, spawn_menu_bar,
    sync_menu_popup_visibility_system, update_strategy_status_label_system,
};
use crate::ui::orders::orders_panel_system;
use crate::ui::positions::positions_panel_system;
use crate::ui::run_result_panel::run_result_panel_system;
use crate::ui::components::{
    mark_registry_dirty_system, sync_registry_from_scenario_loaded_system,
    sync_scenario_metadata_from_registry_system, writeback_scenario_instruments_system,
};
use crate::ui::scenario_parser::parse_scenario_system;
use crate::ui::window::instrument_chart_sync_system;
use crate::ui::sidebar::{
    instrument_remove_button_system, panel_button_system, spawn_sidebar, update_sidebar_system,
};
use crate::ui::strategy_editor::{
    StrategyAutoSaveState, apply_pending_app_edits_system, apply_strategy_snapshot_restore_system,
    debounced_strategy_autosave_system, sync_editor_to_strategy_buffer_system,
    sync_strategy_buffer_to_editor_system, undo_redo_system, update_strategy_editor_zoom_system,
};
use crate::ui::systems::{button_system, update_price_display, update_status_indicator};
use bevy::prelude::*;
use bevy_cosmic_edit::{CosmicEditPlugin, CosmicFontConfig, prelude::change_active_editor_sprite};
use bevy_vector_shapes::Shape2dPlugin;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            Shape2dPlugin::default(),
            CosmicEditPlugin {
                font_config: CosmicFontConfig::default(),
            },
            crate::ui::layout_persistence::LayoutPersistencePlugin,
        ))
        .init_resource::<WindowManager>()
        .init_resource::<StrategyBuffer>()
        .init_resource::<StrategyAutoSaveState>()
        .init_resource::<RegionKeyAllocator>()
        .init_resource::<PendingStrategyFragments>()
        .init_resource::<AppHistory>()
        .init_resource::<ActiveDrag>()
        .init_resource::<PendingStrategySnapshotRestore>()
        .init_resource::<OpenMenu>()
        .add_event::<StrategyFileLoadRequested>()
        .add_event::<StrategyRunRequested>()
        .add_event::<PanelSpawnRequested>()
        .add_event::<UndoRedoApplied>()
        .add_event::<UndoMenuRequested>()
        .add_event::<RedoMenuRequested>()
        .init_resource::<ScenarioMetadata>()
        .init_resource::<InstrumentRegistry>()
        .init_resource::<ScenarioFileWatchState>()
        .init_resource::<ScenarioInstrumentsWritebackState>()
        .insert_resource(ScenarioWritebackPaths {
            cache_sidecar: crate::ui::menu_bar::cache_state_paths().map(|(json, _)| json),
        })
        .add_event::<ScenarioLoadedFromFile>()
        .add_systems(
            Startup,
            (
                spawn_footer,
                spawn_menu_bar,
                spawn_sidebar,
                // 起動時に固定 cache から復元する（CacheRestoreRequested 発火）
                restore_last_strategy_system,
            ),
        )
        .add_systems(
            Update,
            (
                update_price_display,
                button_system,
                update_status_indicator,
                chart_render_system,
                update_footer_system,
                transport_button_system,
                footer_pause_resume_system.before(handle_strategy_run_system),
                speed_button_system,
                update_speed_buttons_system,
                log_strategy_file_load_requested_system,
                handle_strategy_file_load_system,
                update_strategy_status_label_system,
                run_result_panel_system,
                log_strategy_run_requested_system,
                handle_strategy_run_system.after(sync_scenario_metadata_from_registry_system),
                (
                    parse_scenario_system,
                    sync_registry_from_scenario_loaded_system,
                    mark_registry_dirty_system,
                    sync_scenario_metadata_from_registry_system,
                    writeback_scenario_instruments_system,
                    instrument_chart_sync_system,
                )
                    .chain(),
                update_sidebar_system,
                instrument_remove_button_system,
                panel_button_system,
                panel_spawn_dispatcher_system,
            ),
        )
        .add_systems(
            Update,
            (
                menu_top_level_system,
                menu_item_system,
                menu_keyboard_system,
                sync_menu_popup_visibility_system,
            ),
        )
        .add_systems(
            Update,
            (
                buying_power_panel_system,
                positions_panel_system,
                orders_panel_system,
                sync_editor_to_strategy_buffer_system,
                undo_redo_system.after(sync_editor_to_strategy_buffer_system),
                apply_pending_app_edits_system.after(undo_redo_system),
                apply_strategy_snapshot_restore_system.after(apply_pending_app_edits_system),
                sync_strategy_buffer_to_editor_system
                    .after(handle_strategy_file_load_system)
                    .after(apply_pending_app_edits_system)
                    .after(apply_strategy_snapshot_restore_system),
                debounced_strategy_autosave_system,
                update_strategy_editor_zoom_system,
            ),
        )
        .add_systems(
            Update,
            change_active_editor_sprite.after(menu_keyboard_system),
        );
    }
}
