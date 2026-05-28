pub mod buying_power;
pub mod chart_axes;
pub mod chart_crosshair;
pub mod chart_interaction;
pub mod chart_ladder_pane;
pub mod chart_render;
pub mod chart_viewstate;
pub mod chart_volume;
pub mod components;
pub mod editor_history;
pub mod floating_window;
pub mod footer;
pub mod instrument_picker;
pub mod instruments_universe_prune;
pub mod layout_persistence;
pub mod menu_bar;
pub mod modify_modal;
pub mod order_context_menu;
pub mod order_panel;
pub mod orders;
pub mod positions;
pub mod reconcile_modal;
pub mod relogin_modal;
pub mod render_scale;
pub mod replay_startup_window;
pub mod restore;
pub mod run_result_panel;
pub mod safety_toast;
pub mod scenario_parser;
pub mod scenario_startup_panel;
pub mod secret_modal;
pub mod settings;
pub mod sidebar;
pub mod strategy_editor;
pub mod strategy_editor_compose;
pub mod strategy_editor_find;
pub mod strategy_editor_gutter;
pub mod strategy_editor_highlight;
pub mod strategy_editor_input;
pub mod strategy_editor_scrollbar;
// issue #50 Step 0 spike — cosmic 並存の Projected Node PoC（Go/No-Go ゲート）。
pub mod strategy_editor_spike;
pub mod systems;
pub mod window;

pub use components::{
    ChartInstrument, InstrumentRegistry, ScenarioFileWatchState, ScenarioInstrumentsWritebackState,
    ScenarioLoadedFromFile, ScenarioReadTarget, ScenarioWritebackPaths,
};
pub use render_scale::{RenderScaleResponsive, update_cosmic_render_scale_system};

use crate::ui::buying_power::buying_power_panel_system;
use crate::ui::chart_axes::{price_axis_labels_system, time_axis_labels_system};
use crate::ui::chart_crosshair::{
    chart_crosshair_derive_system, chart_crosshair_render_system, crosshair_badge_system,
    install_chart_crosshair_observer,
};
use crate::ui::chart_interaction::{
    ChartClickState, chart_click_state_cleanup_system, chart_scroll_zoom_system,
    install_chart_autoscale_reset_observer, install_chart_drag_observer,
};
use crate::ui::chart_ladder_pane::{chart_ladder_mode_sync_system, ladder_render_system};
use crate::ui::chart_render::chart_main_render_system;
use crate::ui::chart_viewstate::{
    ChartSet, RequestAutoscale, chart_autoscale_apply_system, chart_data_tick_system,
    chart_interaction_tick_system,
};
use crate::ui::chart_volume::volume_render_system;
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
use crate::ui::components::{
    ScenarioStartupParams, SidebarTickersScrollOffset, SidebarTickersSearchState,
};
use crate::ui::components::ChartSizeMap;
use crate::ui::editor_history::{
    ActiveDrag, AppHistory, PendingStrategySnapshotRestore, UndoRedoApplied,
};
use crate::ui::floating_window::{floating_window_layout_system, panel_spawn_dispatcher_system};
use crate::ui::footer::{
    execution_mode_toggle_system, footer_pause_resume_system, spawn_footer, speed_button_system,
    transport_button_system, update_footer_system, update_speed_buttons_system,
};
use crate::ui::instrument_picker::{
    add_instrument_button_system, auto_fetch_available_on_replay_entry_system,
    auto_fetch_live_universe_on_connect_system, force_close_picker_on_lock_system,
    picker_list_rebuild_system, picker_row_click_system, picker_searchbox_input_system,
    retry_pending_live_universe_system, subscribe_added_instruments_system,
    sync_picker_dropdown_visibility_system,
};
use crate::ui::instruments_universe_prune::{
    invalidate_tickers_on_venue_disconnect_system, prune_instruments_outside_universe_system,
    unsubscribe_removed_instruments_system,
};
use crate::ui::menu_bar::{
    gate_venue_menu_items_system, handle_strategy_file_load_system, handle_strategy_run_system,
    hide_unconfigured_venue_items_system, log_strategy_file_load_requested_system,
    log_strategy_run_requested_system, menu_item_system, menu_keyboard_system,
    menu_top_level_system, restore_last_strategy_system, spawn_menu_bar,
    sync_menu_popup_visibility_system, update_strategy_status_label_system,
};
use crate::ui::modify_modal::{
    ModifyForm, modify_modal_button_system, modify_modal_input_system, modify_modal_sync_system,
    modify_modal_visibility_system, spawn_modify_modal,
};
use crate::ui::order_context_menu::{
    OrderContextMenu, context_menu_hover_system, context_menu_item_system,
    context_menu_keyboard_system, context_menu_visibility_system, spawn_order_context_menu,
};
use crate::ui::order_panel::{
    OrderButtonPressed, OrderConfirm, OrderForm, confirm_modal_button_system,
    confirm_modal_sync_system, confirm_modal_visibility_system, order_form_button_system,
    order_panel_sync_system, order_submit_button_system,
    order_window_despawn_system, spawn_confirm_modal,
};
use crate::ui::orders::orders_panel_system;
use crate::ui::positions::positions_panel_system;
use crate::ui::reconcile_modal::{
    reconcile_modal_button_system, reconcile_modal_sync_system, reconcile_modal_visibility_system,
    spawn_reconcile_modal,
};
use crate::ui::relogin_modal::{
    relogin_modal_button_system, relogin_modal_sync_system, relogin_modal_visibility_system,
    spawn_relogin_modal,
};
use crate::ui::restore::restore_fixed_registry_on_replay_entry_system;
use crate::ui::run_result_panel::{
    apply_run_result_visibility_system, run_result_panel_system,
    spawn_run_result_panel_system,
};
use crate::ui::safety_toast::{safety_toast_system, spawn_safety_toast};
use crate::ui::scenario_parser::parse_scenario_system;
use crate::ui::scenario_startup_panel::{
    ScenarioStartupParamCommit, commit_startup_params_to_scenario_system,
    enforce_scenario_startup_panel_readonly_system,
    scenario_startup_param_input_system, spawn_scenario_startup_input_fields,
    spawn_scenario_startup_window_system, sync_startup_param_editors_text_system,
    sync_startup_params_from_scenario_system, update_scenario_startup_param_ui_system,
    write_startup_params_to_cache_sidecar_system,
};
use crate::ui::secret_modal::{
    SecretInput, secret_modal_button_system, secret_modal_input_system,
    secret_modal_lifecycle_system, secret_modal_sync_system, secret_modal_timeout_system,
    secret_modal_visibility_system, spawn_secret_modal,
};
use crate::ui::sidebar::{
    instrument_remove_button_system, instrument_row_click_system, panel_button_system,
    spawn_sidebar, update_instrument_price_text_system, update_sidebar_system,
};
use crate::ui::strategy_editor::{
    StrategyAutoSaveState, apply_pending_app_edits_system, apply_strategy_snapshot_restore_system,
    debounced_strategy_autosave_system, strategy_editor_content_layout_system,
    sync_editor_to_strategy_buffer_system, sync_strategy_buffer_to_editor_system, undo_redo_system,
};
use crate::ui::strategy_editor_compose::apply_highlight_layers_system;
use crate::ui::strategy_editor_find::{
    FindActionRequested, FindReplaceState, compute_find_match_spans_system,
    find_button_interaction_system, find_field_input_system, find_keyboard_system,
    find_navigate_system, manage_find_panel_lifecycle_system, replace_execute_system,
    update_find_count_text_system,
};
use crate::ui::strategy_editor_gutter::{sync_gutter_scroll_system, update_gutter_text_system};
use crate::ui::strategy_editor_highlight::{
    compute_bracket_spans_system, compute_syntax_spans_system, init_syntect_highlighter,
};
use crate::ui::strategy_editor_input::{
    bracket_autoclose_system, enter_autoindent_system, tab_input_system,
};
use crate::ui::strategy_editor_scrollbar::update_scrollbar_thumb_system;
use crate::ui::systems::{update_price_display, update_status_indicator};
use crate::ui::window::{chart_content_layout_system, instrument_chart_sync_system};
use bevy::prelude::*;
use bevy_cosmic_edit::{
    CosmicEditPlugin, CosmicFontConfig,
    prelude::{change_active_editor_sprite, change_active_editor_ui},
};
use bevy_vector_shapes::Shape2dPlugin;

pub struct UiPlugin;

/// mode 可視性 system 群の登録。production と RED ガード (M20) が同一 registration を共有する。
pub fn add_mode_visibility_systems(app: &mut App) {
    // 全 mode 可視性 system は ExecutionModeRes の唯一の writer (status_update_system) の後に走らせ、mode 遷移フレームで 1 フレーム古い可視性を出さない（race-free。issue #41）。
    app.add_systems(
        Update,
        (
            crate::ui::footer::apply_venue_live_button_visibility_system
                .after(crate::backend_sync::status_update_system),
            crate::ui::footer::apply_execution_mode_visibility_system
                .after(crate::backend_sync::status_update_system),
            crate::ui::scenario_startup_panel::apply_startup_panel_visibility_system
                .after(crate::backend_sync::status_update_system),
            apply_run_result_visibility_system
                .after(panel_spawn_dispatcher_system)
                .after(crate::backend_sync::status_update_system),
            // issue #31: layout apply / panel spawn の後に走らせ、Manual 中の新規 spawn 窓を退避マーカーへ捕捉する（flash 防止 + マーカー陳腐化防止）。
            crate::ui::strategy_editor::apply_strategy_editor_mode_visibility_system
                .after(crate::ui::layout_persistence::apply_layout_system)
                .after(crate::ui::layout_persistence::apply_pending_layout_system)
                .after(panel_spawn_dispatcher_system)
                .after(crate::backend_sync::status_update_system),
            crate::ui::sidebar::apply_order_button_visibility_system
                .after(crate::backend_sync::status_update_system),
        ),
    );
}

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            Shape2dPlugin::default(),
            CosmicEditPlugin {
                font_config: CosmicFontConfig::default(),
            },
            crate::ui::layout_persistence::LayoutPersistencePlugin,
            // issue #50 Step 0 spike: bevscode editor を Projected Node 方式で世界座標に出すための plugins。
            // cosmic_edit と並存（spike が Go になるまで撤去しない）。
            bevscode::prelude::CodeEditorPlugins,
        ))
        .add_message::<crate::ui::strategy_editor_spike::SpikeEditorSpawnRequested>()
        .init_resource::<WindowManager>()
        .init_resource::<StrategyBuffer>()
        .init_resource::<StrategyAutoSaveState>()
        .init_resource::<RegionKeyAllocator>()
        .init_resource::<PendingStrategyFragments>()
        .init_resource::<AppHistory>()
        .init_resource::<ActiveDrag>()
        .init_resource::<PendingStrategySnapshotRestore>()
        .init_resource::<FindReplaceState>()
        .add_message::<FindActionRequested>()
        .add_message::<OrderButtonPressed>()
        // ⚠️ 必須: chart_data_tick_system が EventWriter<RequestAutoscale> を取るので
        //    Events リソースが要る。未登録だと初回取得で panic する。
        .add_message::<RequestAutoscale>()
        // Phase E: double-click reset observer が per-chart クリック状態を持つ。
        .init_resource::<ChartClickState>()
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
        .add_message::<StrategyFileLoadRequested>()
        .add_message::<StrategyRunRequested>()
        .add_message::<PanelSpawnRequested>()
        .add_message::<UndoRedoApplied>()
        .add_message::<UndoMenuRequested>()
        .add_message::<RedoMenuRequested>()
        .add_message::<ScenarioStartupParamCommit>()
        .init_resource::<ScenarioMetadata>()
        .init_resource::<ScenarioStartupParams>()
        .init_resource::<InstrumentRegistry>()
        .init_resource::<ChartSizeMap>()
        .init_resource::<SidebarTickersScrollOffset>()
        .init_resource::<SidebarTickersSearchState>()
        .init_resource::<ScenarioFileWatchState>()
        .init_resource::<ScenarioReadTarget>()
        .init_resource::<ScenarioInstrumentsWritebackState>()
        .insert_resource(ScenarioWritebackPaths {
            cache_sidecar: crate::ui::menu_bar::cache_state_paths().map(|(json, _)| json),
        })
        .add_message::<ScenarioLoadedFromFile>()
        .add_message::<ScenarioClearedFromFile>()
        // Phase 9 §3.9 / §3.10: OrderPanel form state + 2-stage confirm + Secret input.
        // `SecretPrompt` / `LiveOrders` are inserted in the binary (main.rs) since the
        // transport-facing systems own them.
        .init_resource::<OrderForm>()
        .init_resource::<OrderConfirm>()
        .init_resource::<SecretInput>()
        // Phase 9 §3.11 / §3.12 (Step 4): right-click context menu + Modify modal.
        .init_resource::<OrderContextMenu>()
        .init_resource::<ModifyForm>()
        // Phase 10 §2.9: OrdersPanel strategy_id filter (All / Manual / Strategy).
        .init_resource::<crate::trading::OrdersFilter>()
        // Phase 10 §2.10 / log Open Question: violation toast + strategy log buffer.
        .init_resource::<crate::trading::SafetyToast>()
        .init_resource::<crate::trading::StrategyLogs>()
        .add_systems(
            Startup,
            (
                spawn_footer,
                spawn_menu_bar,
                spawn_sidebar,
                spawn_scenario_startup_window_system,
                spawn_scenario_startup_input_fields.after(spawn_scenario_startup_window_system),
                // 起動時に固定 cache から復元する（CacheRestoreRequested 発火）
                restore_last_strategy_system,
                // highlight pipeline: syntect SyntaxSet/Theme を resource として用意
                init_syntect_highlighter,
                // Phase 9: LiveManual 発注 UI (floating window 流派)
                spawn_confirm_modal,
                spawn_secret_modal,
                // Phase 9 Step 4: 右クリックコンテキストメニュー + Modify モーダル
                spawn_order_context_menu,
                spawn_modify_modal,
                // Phase 9 Step 7: 再ログイン通知モーダル (venue 本体ログアウト検知)
                spawn_relogin_modal,
                // Phase 9 Step 8 §3.8: backend 再起動後の注文 reconcile 通知モーダル
                spawn_reconcile_modal,
                // Phase 10 §2.10: Safety Rail violation toast (Footer 右下)
                spawn_safety_toast,
                spawn_run_result_panel_system,
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
                floating_window_layout_system,
                strategy_editor_content_layout_system,
                chart_content_layout_system,
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
                // Phase C: pan/zoom。observer 設置は schedule 外発火なので set 無し。
                // scroll zoom は cursor 補正で最新 base_price_y を読むので Interaction (after Autoscale)。
                install_chart_drag_observer,
                // Phase E: double-click で pan/zoom リセット + autoscale 再有効化。observer 設置は
                // schedule 外発火なので set 無し。
                install_chart_autoscale_reset_observer,
                // despawn された chart の ChartClickState エントリを掃除 (entity key leak 防止)。
                chart_click_state_cleanup_system,
                chart_scroll_zoom_system.in_set(ChartSet::Interaction),
                chart_main_render_system.in_set(ChartSet::Render),
                // Phase E: volume サブペイン。immediate-mode 純 draw (Changed gate しない)。
                volume_render_system.in_set(ChartSet::Render),
                // Phase B: axis label は Changed<ChartViewState> 駆動の retained Text2d なので
                // Render set (autoscale 確定後) に置く。
                // ⚠️ instrument_chart_sync_system の後に置く: chart panel が prune→sync で despawn
                //    される frame に、gutter spawn が flush される前に set_parent すると panic する。
                //    sync の後なら despawn 済 chart は Changed query に出ず gutter も生存が保証される。
                price_axis_labels_system
                    .in_set(ChartSet::Render)
                    .after(instrument_chart_sync_system),
                time_axis_labels_system
                    .in_set(ChartSet::Render)
                    .after(instrument_chart_sync_system),
                // Phase D: crosshair。observer 設置は schedule 外発火なので set 無し。
                install_chart_crosshair_observer,
                // derive は autoscale 確定後の base_price_y/cell_height で readout を計算 (Render)。
                chart_crosshair_derive_system.in_set(ChartSet::Render),
                // cross line は毎フレーム純 draw (immediate-mode、Changed gate しない)。
                chart_crosshair_render_system.in_set(ChartSet::Render),
                // badge は derive 後 (hovered_price/time を読む) かつ sync 後 (gutter set_parent panic 回避)。
                crosshair_badge_system
                    .in_set(ChartSet::Render)
                    .after(chart_crosshair_derive_system)
                    .after(instrument_chart_sync_system),
            ),
        )
        // ── Chart (Phase 7.3 F): Live モード複合ウィンドウ (ローソク足＋Ladder) ──
        .add_systems(
            Update,
            (
                // ExecutionMode 変化 / Added<WindowRoot> で Ladder spawn/despawn + 枠リサイズ + chart 左シフト。
                // ⚠️ instrument_chart_sync_system の後: prune→sync で despawn される frame に
                //    despawn 済 content_area へ set_parent すると panic する (Caveat #26 と同根)。
                chart_ladder_mode_sync_system
                    .after(crate::trading::backend_update_system)
                    .after(instrument_chart_sync_system),
                // per-instrument depth → 行生成。mode_sync の後 (新規 pane が flush 済みになってから読む)。
                ladder_render_system.after(chart_ladder_mode_sync_system),
            ),
        )
        .add_systems(
            Update,
            (
                (
                    scenario_startup_param_input_system,
                    parse_scenario_system,
                    sync_registry_from_scenario_loaded_system,
                    sync_registry_from_scenario_cleared_system,
                    // §5.3 順序: status 更新後、Tickers 鮮度リセット → live universe fetch
                    invalidate_tickers_on_venue_disconnect_system,
                    auto_fetch_live_universe_on_connect_system,
                    // #32 Slice 2: warming 中（PENDING→InFlight）は store 充填まで再 fetch
                    retry_pending_live_universe_system,
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
                update_cosmic_render_scale_system,
            ),
        )
        .add_systems(
            Update,
            (change_active_editor_sprite, change_active_editor_ui)
                .after(menu_keyboard_system)
                .after(picker_searchbox_input_system),
        );
        // mode 可視性 system 群（footer/startup/run_result/strategy_editor/order）は
        // production と M20 RED ガードで同一 registration を共有するため関数に切り出す。
        add_mode_visibility_systems(app);
        app
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
        // ── gutter + scrollbar (Phase B) ──
        // gutter テキストは Changed<StrategyFragment> 駆動。scroll 追従とサムは
        // エディタの scroll を読むだけなので毎フレーム回す (1 フレーム遅延は不可視)。
        .add_systems(
            Update,
            (
                update_gutter_text_system,
                sync_gutter_scroll_system,
                update_scrollbar_thumb_system,
            ),
        )
        // ── Tab / Enter / bracket autoclose (Phase C) ──
        // Tab/Enter は cosmic より先に走って reset で抑止 (.before)。
        // bracket closer は cosmic が opener を入れた直後 (.after)。
        .add_systems(
            Update,
            (
                tab_input_system.before(bevy_cosmic_edit::InputSet),
                enter_autoindent_system.before(bevy_cosmic_edit::InputSet),
                bracket_autoclose_system.after(bevy_cosmic_edit::InputSet),
            ),
        )
        // ── Find / Replace パネル (Slice 5 #50: Bevy UI Node 化、cosmic 経路撤去) ──
        // マッチ計算は composer の前 (FindMatchSpans を書く)。色付けは composer (Slice 6 で削除)。
        .add_systems(
            Update,
            (
                find_keyboard_system.before(manage_find_panel_lifecycle_system),
                manage_find_panel_lifecycle_system,
                // Bevy UI Node 入力: keyboard drain (focused_field=Query/Replacement 時)
                find_field_input_system.after(manage_find_panel_lifecycle_system),
                // ボタン Interaction エッジ → FindActionRequested
                find_button_interaction_system.after(manage_find_panel_lifecycle_system),
                compute_find_match_spans_system
                    .after(find_field_input_system)
                    .before(apply_highlight_layers_system),
                find_navigate_system
                    .after(compute_find_match_spans_system)
                    .after(find_button_interaction_system)
                    .before(apply_highlight_layers_system),
                // replace は composer の後。bevscode SetTextRequested を発行するだけ
                // (TextBuffer ↔ fragment/autosave は sync_bevscode_to_strategy_fragment_system が駆動)。
                replace_execute_system
                    .after(find_button_interaction_system)
                    .after(apply_highlight_layers_system),
                // 件数表示はマッチ確定 (compute) とナビ確定 (navigate) の後に読む。
                update_find_count_text_system
                    .after(compute_find_match_spans_system)
                    .after(find_navigate_system),
            ),
        )
        // ── Phase 9: OrderPanel (LiveManual 手動発注) + 2 段階確認 + SecretModal ──
        .add_systems(
            Update,
            (
                // OrderPanel
                order_form_button_system,
                order_submit_button_system,
                order_panel_sync_system,
                order_window_despawn_system,
                confirm_modal_visibility_system,
                // §3.10 Escape determinism: the confirm modal yields its Escape to an
                // open SecretModal. Because SecretModal consumes Escape via its event
                // drain (not ButtonInput), this system must read `secret_prompt.active`
                // BEFORE the drain clears it — so run `.before(secret_modal_input_system)`.
                confirm_modal_button_system.before(secret_modal_input_system),
                confirm_modal_sync_system,
                // SecretModal — input は cosmic より先に走って keystroke を消費する
                // (picker_searchbox と同じ drain パターン)。最前面オーバーレイ (z=300) なので
                // picker / menu の drain より先に走らせ、同フレーム共存時もモーダルが入力を得る。
                secret_modal_lifecycle_system,
                secret_modal_visibility_system,
                secret_modal_input_system
                    .before(bevy_cosmic_edit::InputSet)
                    .before(picker_searchbox_input_system)
                    .before(menu_keyboard_system),
                secret_modal_button_system,
                secret_modal_timeout_system,
                secret_modal_sync_system,
            ),
        )
        // ── Phase 9 Step 4: OrdersPanel 右クリックメニュー + Modify モーダル ──
        .add_systems(
            Update,
            (
                // Context menu (右クリック → [取消]/[訂正])
                context_menu_visibility_system,
                // §3.10 Escape determinism (see confirm_modal_button_system): this
                // notice reader yields Escape to a higher-priority modal, so it must
                // read those flags BEFORE they are cleared — run before both the
                // SecretModal drain and the confirm-modal button system.
                context_menu_keyboard_system
                    .before(secret_modal_input_system)
                    .before(confirm_modal_button_system),
                context_menu_item_system,
                context_menu_hover_system,
                // Modify modal — input は cosmic / picker / menu より先に keystroke を消費する
                // (secret_modal と同じ drain パターン)。secret modal (最前面・最優先) が同フレームに
                // 開いている稀ケースでは secret 側が先に drain するよう `.after(secret_modal_input_system)`
                // を付け、決定的にする (両者が同じ keyboard event を奪い合うのを防ぐ)。
                modify_modal_visibility_system,
                modify_modal_input_system
                    .after(secret_modal_input_system)
                    .before(bevy_cosmic_edit::InputSet)
                    .before(picker_searchbox_input_system)
                    .before(menu_keyboard_system),
                modify_modal_button_system,
                modify_modal_sync_system,
            ),
        )
        // ── Phase 9 Step 7: 再ログイン通知モーダル (venue 本体ログアウト検知, §3.5) ──
        .add_systems(
            Update,
            (
                relogin_modal_visibility_system,
                // §3.10 Escape determinism (see context_menu_keyboard_system).
                relogin_modal_button_system
                    .before(secret_modal_input_system)
                    .before(confirm_modal_button_system),
                relogin_modal_sync_system,
            ),
        )
        // ── Phase 9 Step 8 §3.8: backend 再起動後の注文 reconcile 通知モーダル ──
        .add_systems(
            Update,
            (
                reconcile_modal_visibility_system,
                // §3.10 Escape determinism (see context_menu_keyboard_system).
                reconcile_modal_button_system
                    .before(secret_modal_input_system)
                    .before(confirm_modal_button_system),
                reconcile_modal_sync_system,
            ),
        )
        // ── Phase 10 §2.10: Safety Rail violation toast ──
        .add_systems(Update, safety_toast_system)
        // ── issue #50 Step 0 spike: Projected Node 方式 PoC ──
        // ADR 0006 / plan: cosmic_edit と並存させて Go/No-Go を判定する。
        // - handle_spike_editor_spawn_requests: メニュー / キーボードからの spawn 要求を受ける（Update）
        // - cleanup_spike_editor_on_root_despawn: × で root が消えたら Node も連れて despawn（Update）
        .add_systems(
            Update,
            (
                crate::ui::strategy_editor_spike::handle_spike_editor_spawn_requests,
                crate::ui::strategy_editor_spike::cleanup_spike_editor_on_root_despawn,
            ),
        )
        // - project_spike_editor_node_system: world rect → screen rect 投影で Node を毎フレーム更新
        //   （`UiSystems::Layout` の前 = 同フレーム ComputedNode に反映）。
        // - touch_spike_text_layouts_on_position_change: drag/pan で editor が動くだけ
        //   （size 不変）のケースで bevy_instanced_text の `produce_layouts` / `update_text_views`
        //   がキャッシュで早期 return するのを回避するため、`Changed<UiGlobalTransform>` を検知
        //   して editor / gutter の `DisplayLayout` を `set_changed()` する。詳細は spike module
        //   の docstring（issue #50 Step 0 / ADR 0006 / bevy_instanced_text 51220b7 の挙動）。
        //   順序は `LayoutProduceSet.after`（produce_layouts の上書きを避ける）/
        //   `TextViewRenderSet.before`（update_text_views が拾えるタイミング）。
        .add_systems(
            PostUpdate,
            (
                crate::ui::strategy_editor_spike::project_spike_editor_node_system
                    .before(bevy::ui::UiSystems::Layout),
                crate::ui::strategy_editor_spike::touch_spike_text_layouts_on_position_change
                    .after(bevy_instanced_text::LayoutProduceSet)
                    .before(bevy_instanced_text::TextViewRenderSet),
            ),
        )
        // ── Slice 1 (#50): bevscode 本実装 Projected Node 系統（cosmic と並存）──
        // ADR 0006. spike と同じ schedule 構成で本実装 marker（StrategyEditorRoot / StrategyEditorNode）に
        // 適用する。Slice 7 で spike 撤去後はこちらだけが残る。
        // - spawn_bevscode_peer_on_strategy_editor_added: Added<StrategyEditorRoot> を watch し peer を生成
        // - apply_pending_strategy_seed_system: seed を SetTextRequested で投入し pending marker を外す
        // - cleanup_strategy_editor_node_on_root_despawn: root が消えたら peer を片付ける
        .add_systems(
            Update,
            (
                crate::ui::strategy_editor::spawn_bevscode_peer_on_strategy_editor_added,
                crate::ui::strategy_editor::apply_pending_strategy_seed_system,
                crate::ui::strategy_editor::cleanup_strategy_editor_node_on_root_despawn,
            ),
        )
        // ── Slice 2 (#50): bevscode ↔ StrategyBuffer 同期 + AppHistory undo bridge ──
        // - install_strategy_editor_keybindings: bevscode 既定の Ctrl+Z/Y を剥がした InputMap を
        //   Startup で先取り spawn（PostStartup の `spawn_default_input_manager` は既存があればスキップ）
        // - sync_bevscode_to_strategy_fragment_system: bevscode 入力 → StrategyFragment + autosave + AppHistory
        // - sync_strategy_fragment_to_bevscode_system: AppHistory writeback / file load → bevscode SetTextRequested
        .add_systems(
            Startup,
            crate::ui::strategy_editor::install_strategy_editor_keybindings,
        )
        .add_systems(
            Update,
            (
                crate::ui::strategy_editor::sync_bevscode_to_strategy_fragment_system,
                crate::ui::strategy_editor::sync_strategy_fragment_to_bevscode_system,
            ),
        )
        // - project_strategy_editor_node_system: world rect → screen rect 投影で Node を毎フレーム更新
        // - touch_strategy_text_layouts_on_position_change: drag/pan で動いた editor の DisplayLayout を
        //   set_changed() して glyph batch キャッシュバグを回避（spike と同じ理由・同じ schedule）
        .add_systems(
            PostUpdate,
            (
                crate::ui::strategy_editor::project_strategy_editor_node_system
                    .before(bevy::ui::UiSystems::Layout),
                crate::ui::strategy_editor::touch_strategy_text_layouts_on_position_change
                    .after(bevy_instanced_text::LayoutProduceSet)
                    .before(bevy_instanced_text::TextViewRenderSet),
            ),
        );
    }
}
