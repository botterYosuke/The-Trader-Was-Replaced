pub mod buying_power;
pub mod chart_render;
pub mod chart_viewstate;
pub mod components;
pub mod editor_history;
pub mod floating_window;
pub mod footer;
pub mod instrument_picker;
pub mod instruments_universe_prune;
pub mod layout_persistence;
pub mod menu_bar;
pub mod orders;
pub mod positions;
pub mod replay_startup_window;
pub mod restore;
pub mod run_result_panel;
pub mod scenario_parser;
pub mod scenario_startup_panel;
pub mod sidebar;
pub mod strategy_editor;
pub mod strategy_editor_highlight;
pub mod strategy_editor_compose;
pub mod strategy_editor_find;
pub mod systems;
pub mod window;

pub use components::{
    ChartInstrument, InstrumentRegistry, ScenarioFileWatchState, ScenarioInstrumentsWritebackState,
    ScenarioLoadedFromFile, ScenarioReadTarget, ScenarioWritebackPaths,
};

use crate::ui::buying_power::buying_power_panel_system;
use crate::ui::chart_render::chart_main_render_system;
use crate::ui::chart_viewstate::{
    ChartSet, RequestAutoscale, chart_autoscale_apply_system, chart_data_tick_system,
    chart_interaction_tick_system,
};
use crate::ui::components::{
    OpenMenu, PanelSpawnRequested, PendingStrategyFragments, RedoMenuRequested, RegionKeyAllocator,
    ScenarioMetadata, StrategyBuffer, StrategyFileLoadRequested, StrategyRunRequested,
    UndoMenuRequested, WindowManager,
};
use crate::ui::components::{
    ScenarioClearedFromFile, mark_registry_dirty_system,
    sync_registry_from_scenario_cleared_system, sync_registry_from_scenario_loaded_system,
    sync_scenario_metadata_from_registry_system, writeback_scenario_instruments_system,
};
use crate::ui::editor_history::{
    ActiveDrag, AppHistory, PendingStrategySnapshotRestore, UndoRedoApplied,
};
use crate::ui::floating_window::panel_spawn_dispatcher_system;
use crate::ui::footer::{
    execution_mode_toggle_system, footer_pause_resume_system, spawn_footer, speed_button_system,
    transport_button_system, update_footer_system, update_speed_buttons_system,
};
use crate::ui::instrument_picker::{
    add_instrument_button_system, auto_fetch_available_on_replay_entry_system,
    auto_fetch_live_universe_on_connect_system, force_close_picker_on_lock_system,
    picker_list_rebuild_system, picker_row_click_system, picker_searchbox_input_system,
    subscribe_added_instruments_system, sync_picker_dropdown_visibility_system,
};
use crate::ui::instruments_universe_prune::{
    invalidate_tickers_on_venue_disconnect_system, prune_instruments_outside_universe_system,
    unsubscribe_removed_instruments_system,
};
use crate::ui::restore::restore_fixed_registry_on_replay_entry_system;
use crate::ui::menu_bar::{
    gate_venue_menu_items_system, hide_unconfigured_venue_items_system,
    handle_strategy_file_load_system, handle_strategy_run_system,
    log_strategy_file_load_requested_system, log_strategy_run_requested_system, menu_item_system,
    menu_keyboard_system, menu_top_level_system, restore_last_strategy_system, spawn_menu_bar,
    sync_menu_popup_visibility_system, update_strategy_status_label_system,
};
use crate::ui::orders::orders_panel_system;
use crate::ui::positions::positions_panel_system;
use crate::ui::run_result_panel::run_result_panel_system;
use crate::ui::scenario_parser::parse_scenario_system;
use crate::ui::scenario_startup_panel::{
    ScenarioStartupParamCommit, commit_startup_params_to_scenario_system,
    enforce_scenario_startup_panel_readonly_system, scenario_startup_granularity_button_system,
    scenario_startup_param_input_system, spawn_scenario_startup_input_fields,
    spawn_scenario_startup_panel, sync_startup_param_editors_text_system,
    sync_startup_params_from_scenario_system, update_scenario_startup_param_ui_system,
    write_startup_params_to_cache_sidecar_system,
};
use crate::ui::components::{
    ScenarioStartupParams, SidebarTickersScrollOffset, SidebarTickersSearchState,
};
use crate::ui::sidebar::{
    instrument_remove_button_system, instrument_row_click_system, panel_button_system,
    spawn_sidebar, update_instrument_price_text_system, update_sidebar_system,
};
use crate::ui::strategy_editor::{
    StrategyAutoSaveState, apply_pending_app_edits_system, apply_strategy_snapshot_restore_system,
    debounced_strategy_autosave_system, sync_editor_to_strategy_buffer_system,
    sync_strategy_buffer_to_editor_system, undo_redo_system, update_strategy_editor_zoom_system,
};
use crate::ui::strategy_editor_compose::apply_highlight_layers_system;
use crate::ui::strategy_editor_find::{
    FindActionRequested, FindReplaceState, compute_find_match_spans_system, find_keyboard_system,
    find_navigate_system, find_scroll_to_match_system, manage_find_panel_lifecycle_system,
    replace_execute_system, sync_find_editors_to_state_system, update_find_count_text_system,
};
use crate::ui::strategy_editor_highlight::{
    compute_bracket_spans_system, compute_syntax_spans_system, init_syntect_highlighter,
};
use crate::ui::systems::{update_price_display, update_status_indicator};
use crate::ui::window::instrument_chart_sync_system;
use bevy::prelude::*;
use bevy_cosmic_edit::{
    CosmicEditPlugin, CosmicFontConfig,
    prelude::{change_active_editor_sprite, change_active_editor_ui},
};
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
        .init_resource::<FindReplaceState>()
        .add_event::<FindActionRequested>()
        // ⚠️ 必須: chart_data_tick_system が EventWriter<RequestAutoscale> を取るので
        //    Events リソースが要る。未登録だと初回取得で panic する。
        .add_event::<RequestAutoscale>()
        .configure_sets(
            Update,
            (
                ChartSet::DataTick.after(crate::trading::backend_update_system),
                ChartSet::Autoscale.after(ChartSet::DataTick),
                ChartSet::Interaction.after(ChartSet::Autoscale),
                ChartSet::Render
                    .after(ChartSet::Autoscale)
                    .after(ChartSet::Interaction),
            ),
        )
        .init_resource::<OpenMenu>()
        .init_resource::<crate::ui::instrument_picker::InstrumentPickerState>()
        .add_event::<StrategyFileLoadRequested>()
        .add_event::<StrategyRunRequested>()
        .add_event::<PanelSpawnRequested>()
        .add_event::<UndoRedoApplied>()
        .add_event::<UndoMenuRequested>()
        .add_event::<RedoMenuRequested>()
        .add_event::<ScenarioStartupParamCommit>()
        .init_resource::<ScenarioMetadata>()
        .init_resource::<ScenarioStartupParams>()
        .init_resource::<InstrumentRegistry>()
        .init_resource::<SidebarTickersScrollOffset>()
        .init_resource::<SidebarTickersSearchState>()
        .init_resource::<ScenarioFileWatchState>()
        .init_resource::<ScenarioReadTarget>()
        .init_resource::<ScenarioInstrumentsWritebackState>()
        .insert_resource(ScenarioWritebackPaths {
            cache_sidecar: crate::ui::menu_bar::cache_state_paths().map(|(json, _)| json),
        })
        .add_event::<ScenarioLoadedFromFile>()
        .add_event::<ScenarioClearedFromFile>()
        .add_systems(
            Startup,
            (
                spawn_footer,
                spawn_menu_bar,
                spawn_sidebar,
                spawn_scenario_startup_panel.after(spawn_sidebar),
                spawn_scenario_startup_input_fields.after(spawn_scenario_startup_panel),
                // 起動時に固定 cache から復元する（CacheRestoreRequested 発火）
                restore_last_strategy_system,
                // highlight pipeline: syntect SyntaxSet/Theme を resource として用意
                init_syntect_highlighter,
            ),
        )
        .add_systems(
            Update,
            (
                update_price_display,
                update_status_indicator,
                update_footer_system,
                transport_button_system,
                footer_pause_resume_system.before(handle_strategy_run_system),
                speed_button_system,
                update_speed_buttons_system,
                execution_mode_toggle_system,
                log_strategy_file_load_requested_system,
                handle_strategy_file_load_system,
                update_strategy_status_label_system,
                run_result_panel_system,
                log_strategy_run_requested_system,
                handle_strategy_run_system
                    .after(sync_scenario_metadata_from_registry_system)
                    .after(write_startup_params_to_cache_sidecar_system),
                panel_button_system,
                panel_spawn_dispatcher_system,
            ),
        )
        // ── Chart (Phase 7.3 A): ViewState 更新 / autoscale / 描画 ──
        // observer 系 (Pointer<Drag>/<Move>) は schedule 外なので ChartSet に含めない (Caveat #28)。
        .add_systems(
            Update,
            (
                chart_data_tick_system.in_set(ChartSet::DataTick),
                chart_interaction_tick_system.in_set(ChartSet::DataTick),
                chart_autoscale_apply_system.in_set(ChartSet::Autoscale),
                chart_main_render_system.in_set(ChartSet::Render),
            ),
        )
        .add_systems(
            Update,
            (
                (
                    scenario_startup_param_input_system,
                    scenario_startup_granularity_button_system,
                    parse_scenario_system,
                    sync_registry_from_scenario_loaded_system,
                    sync_registry_from_scenario_cleared_system,
                    // §5.3 順序: status 更新後、Tickers 鮮度リセット → live universe fetch
                    invalidate_tickers_on_venue_disconnect_system,
                    auto_fetch_live_universe_on_connect_system,
                    auto_fetch_available_on_replay_entry_system,
                    // prune → Chart sync → unsubscribe/subscribe → restore → writeback
                    prune_instruments_outside_universe_system,
                    instrument_chart_sync_system,
                    unsubscribe_removed_instruments_system,
                    subscribe_added_instruments_system,
                    restore_fixed_registry_on_replay_entry_system,
                    mark_registry_dirty_system,
                    sync_scenario_metadata_from_registry_system,
                    writeback_scenario_instruments_system,
                )
                    .chain(),
                // ── 新規: scenario startup params (I2b) ──
                (
                    sync_startup_params_from_scenario_system,
                    commit_startup_params_to_scenario_system,
                    write_startup_params_to_cache_sidecar_system,
                    sync_startup_param_editors_text_system,
                    update_scenario_startup_param_ui_system,
                    enforce_scenario_startup_panel_readonly_system,
                )
                    .chain()
                    .after(writeback_scenario_instruments_system),
                update_sidebar_system,
                instrument_remove_button_system,
                instrument_row_click_system.after(update_sidebar_system),
                update_instrument_price_text_system.after(update_sidebar_system),
                add_instrument_button_system.after(sync_registry_from_scenario_loaded_system),
                force_close_picker_on_lock_system.after(mark_registry_dirty_system),
            ),
        )
        .add_systems(
            Update,
            (
                menu_top_level_system,
                menu_item_system,
                menu_keyboard_system,
                sync_menu_popup_visibility_system,
                gate_venue_menu_items_system,
                hide_unconfigured_venue_items_system,
                picker_searchbox_input_system,
                picker_list_rebuild_system
                    .after(picker_searchbox_input_system)
                    .after(force_close_picker_on_lock_system)
                    .after(update_sidebar_system),
                sync_picker_dropdown_visibility_system,
                picker_row_click_system,
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
            (
                change_active_editor_sprite,
                change_active_editor_ui,
            )
                .after(menu_keyboard_system)
                .after(picker_searchbox_input_system),
        )
        .add_systems(
            Update,
            (
                crate::ui::footer::apply_execution_mode_visibility_system,
                crate::ui::scenario_startup_panel::apply_startup_panel_visibility_system,
            ),
        )
        // ── highlight pipeline (Phase A) ──
        // span 計算は buffer→editor 同期の後に走らせ、合成 (apply) はその両者の後。
        .add_systems(
            Update,
            (
                compute_syntax_spans_system
                    .after(sync_strategy_buffer_to_editor_system)
                    .before(apply_highlight_layers_system),
                compute_bracket_spans_system
                    .after(sync_strategy_buffer_to_editor_system)
                    .before(apply_highlight_layers_system),
                apply_highlight_layers_system,
            ),
        )
        // ── Find / Replace パネル (Phase E) ──
        // マッチ計算は composer の前 (FindMatchSpans を書く)。色付けは composer。
        .add_systems(
            Update,
            (
                find_keyboard_system.before(manage_find_panel_lifecycle_system),
                manage_find_panel_lifecycle_system,
                sync_find_editors_to_state_system.after(sync_strategy_buffer_to_editor_system),
                compute_find_match_spans_system
                    .after(sync_find_editors_to_state_system)
                    .before(apply_highlight_layers_system),
                find_navigate_system
                    .after(compute_find_match_spans_system)
                    .before(apply_highlight_layers_system),
                find_scroll_to_match_system.after(find_navigate_system),
                // replace は composer の後。先に走ると set_text 済みの新 buffer に
                // 旧 fragment/旧 spans 由来の attrs を当ててしまう (色は次フレームに再計算)。
                replace_execute_system.after(apply_highlight_layers_system),
                // 件数表示はマッチ確定 (compute) とナビ確定 (navigate) の後に読む。
                update_find_count_text_system
                    .after(compute_find_match_spans_system)
                    .after(find_navigate_system),
            ),
        );
    }
}
