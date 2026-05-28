//! E2E flow runner — one file per flow under `tests/e2e/flows/`, all compiled
//! into this single test binary so `cargo test --test e2e_replay` runs them
//! together (separate `tests/*.rs` files would each become their own binary and
//! link the whole crate). See `tests/e2e/FLOWS.md` for the flow catalog.
//!
//! Only `.rs` files directly under `tests/` form a test crate, so the shared
//! harness and the per-flow files are pulled in via `#[path]` module
//! declarations. Implemented flow files hold exactly one `#[test]`; planned
//! flow files may exist under `tests/e2e/flows/` with only `//!` docs and are
//! not registered here until implementation.

#[path = "e2e/support/mod.rs"]
mod support;
#[path = "e2e/support/ui_dump.rs"]
mod ui_dump;

// A. Replay lifecycle
#[path = "e2e/flows/a1_replay_runs_to_completion.rs"]
mod a1_replay_runs_to_completion;
#[path = "e2e/flows/a2_replay_pause_resume.rs"]
mod a2_replay_pause_resume;
#[path = "e2e/flows/a3_replay_step_forward.rs"]
mod a3_replay_step_forward;
#[path = "e2e/flows/a4_replay_force_stop.rs"]
mod a4_replay_force_stop;
#[path = "e2e/flows/a6_replay_failed_strategy.rs"]
mod a6_replay_failed_strategy;
#[path = "e2e/flows/a12_replay_precision_mismatch_surfaced.rs"]
mod a12_replay_precision_mismatch_surfaced;
#[path = "e2e/flows/a13_replay_play_pause_step_jumptostart.rs"]
mod a13_replay_play_pause_step_jumptostart;
#[path = "e2e/flows/a14_footer_time_advances_on_replay_step.rs"]
mod a14_footer_time_advances_on_replay_step;
#[path = "e2e/flows/a15_replay_step_from_idle.rs"]
mod a15_replay_step_from_idle;
#[path = "e2e/flows/a7_replay_startup_progress.rs"]
mod a7_replay_startup_progress;
#[path = "e2e/flows/a8_stale_startup_id_ignored.rs"]
mod a8_stale_startup_id_ignored;
#[path = "e2e/flows/a9_replay_startup_timeout.rs"]
mod a9_replay_startup_timeout;

// B. Portfolio / run result
#[path = "e2e/flows/b1_portfolio_populated_after_run.rs"]
mod b1_portfolio_populated_after_run;
#[path = "e2e/flows/b2_run_summary_parsed.rs"]
mod b2_run_summary_parsed;

// C. Instrument universe / sidebar
#[path = "e2e/flows/c1_list_instruments_replay.rs"]
mod c1_list_instruments_replay;
#[path = "e2e/flows/c2_list_instruments_failed.rs"]
mod c2_list_instruments_failed;
#[path = "e2e/flows/c3_fetch_available_instruments.rs"]
mod c3_fetch_available_instruments;
#[path = "e2e/flows/c4_fetch_available_failed.rs"]
mod c4_fetch_available_failed;
#[path = "e2e/flows/c6_list_instruments_pending.rs"]
mod c6_list_instruments_pending;

// D. Venue lifecycle (Live)
#[path = "e2e/flows/d1_venue_login_success.rs"]
mod d1_venue_login_success;
#[path = "e2e/flows/d2_venue_subscribed.rs"]
mod d2_venue_subscribed;
#[path = "e2e/flows/d3_venue_login_error.rs"]
mod d3_venue_login_error;
#[path = "e2e/flows/d4_venue_logout.rs"]
mod d4_venue_logout;
#[path = "e2e/flows/d5_venue_logout_detected.rs"]
mod d5_venue_logout_detected;
#[path = "e2e/flows/d6_venue_reconnecting.rs"]
mod d6_venue_reconnecting;
#[path = "e2e/flows/d7_live_universe_overwrite.rs"]
mod d7_live_universe_overwrite;
#[path = "e2e/flows/d9_venue_stays_connected_on_replay_toggle.rs"]
mod d9_venue_stays_connected_on_replay_toggle;
#[path = "e2e/flows/d10_venue_live_buttons_visibility.rs"]
mod d10_venue_live_buttons_visibility;
#[path = "e2e/flows/d11_auto_replay_on_venue_disconnect.rs"]
mod d11_auto_replay_on_venue_disconnect;

// E. Execution mode
#[path = "e2e/flows/e1_set_execution_mode.rs"]
mod e1_set_execution_mode;

// F. Live market data / order & account events
#[path = "e2e/flows/f1_subscribe_market_data.rs"]
mod f1_subscribe_market_data;
#[path = "e2e/flows/f2_unsubscribe_market_data.rs"]
mod f2_unsubscribe_market_data;
#[path = "e2e/flows/f3_order_event.rs"]
mod f3_order_event;
#[path = "e2e/flows/f4_account_event.rs"]
mod f4_account_event;
#[path = "e2e/flows/f5_secret_required.rs"]
mod f5_secret_required;

// G. Backend connection / self-heal
#[path = "e2e/flows/g1_backend_connect_status.rs"]
mod g1_backend_connect_status;
#[path = "e2e/flows/g2_backend_reconnect_selfheal.rs"]
mod g2_backend_reconnect_selfheal;
#[path = "e2e/flows/g3_backend_disabled_sim.rs"]
mod g3_backend_disabled_sim;

// H. Order RPC (live placement / status seam)
#[path = "e2e/flows/h1_order_seeded.rs"]
mod h1_order_seeded;
#[path = "e2e/flows/h2_order_status_updated.rs"]
mod h2_order_status_updated;
#[path = "e2e/flows/h3_order_modified.rs"]
mod h3_order_modified;
#[path = "e2e/flows/h4_order_rejected.rs"]
mod h4_order_rejected;
#[path = "e2e/flows/h5_exec_mode_change_resets_portfolio.rs"]
mod h5_exec_mode_change_resets_portfolio;
#[path = "e2e/flows/h6_order_notice.rs"]
mod h6_order_notice;
#[path = "e2e/flows/h7_secret_submit_failed.rs"]
mod h7_secret_submit_failed;
#[path = "e2e/flows/h9_orders_seeded_full_rows.rs"]
mod h9_orders_seeded_full_rows;
#[path = "e2e/flows/h11_venue_orders_timeout_notice.rs"]
mod h11_venue_orders_timeout_notice;

// I. Menu / file-open / layout (UI / integration)
#[path = "e2e/flows/i1_menu_click_open_close.rs"]
mod i1_menu_click_open_close;
#[path = "e2e/flows/i2_menu_keyboard_alt_shortcuts.rs"]
mod i2_menu_keyboard_alt_shortcuts;
// i3: stub のみ（production に Escape/outside-close handler 未実装）
#[path = "e2e/flows/i4_mode_toggle_client_gating.rs"]
mod i4_mode_toggle_client_gating;
#[path = "e2e/flows/i5_file_open_spawns_editor_and_chart.rs"]
mod i5_file_open_spawns_editor_and_chart;
#[path = "e2e/flows/i6_file_new_resets_loaded_strategy.rs"]
mod i6_file_new_resets_loaded_strategy;
#[path = "e2e/flows/i7_save_layout_writes_sidecar.rs"]
mod i7_save_layout_writes_sidecar;
#[path = "e2e/flows/i8_save_as_writes_new_strategy_pair.rs"]
mod i8_save_as_writes_new_strategy_pair;
#[path = "e2e/flows/i9_file_shortcuts_dispatch.rs"]
mod i9_file_shortcuts_dispatch;
#[path = "e2e/flows/i10_open_live_switches_auto.rs"]
mod i10_open_live_switches_auto;
#[path = "e2e/flows/i11_edit_menu_undo_redo.rs"]
mod i11_edit_menu_undo_redo;
#[path = "e2e/flows/i12_restore_last_strategy_cache_on_launch.rs"]
mod i12_restore_last_strategy_cache_on_launch;
#[path = "e2e/flows/i13_open_scenario_only_json.rs"]
mod i13_open_scenario_only_json;
#[path = "e2e/flows/i14_save_without_path_falls_back_to_dialog.rs"]
mod i14_save_without_path_falls_back_to_dialog;
#[path = "e2e/flows/i15_cache_restore_replay_entry_preserves_py.rs"]
mod i15_cache_restore_replay_entry_preserves_py;
#[path = "e2e/flows/i16_cache_restore_replay_entry_no_inmemory_pollution.rs"]
mod i16_cache_restore_replay_entry_no_inmemory_pollution;
#[path = "e2e/flows/i17_file_open_bad_strategy_path_clears_stale_cache.rs"]
mod i17_file_open_bad_strategy_path_clears_stale_cache;
#[path = "e2e/flows/i18_file_open_relative_strategy_path_loads.rs"]
mod i18_file_open_relative_strategy_path_loads;

// J. Strategy editor / startup panel / scenario / instrument picker (UI / integration)
#[path = "e2e/flows/j1_strategy_editor_text_autosaves_cache.rs"]
mod j1_strategy_editor_text_autosaves_cache;
#[path = "e2e/flows/j2_strategy_editor_tab_indent.rs"]
mod j2_strategy_editor_tab_indent;
#[path = "e2e/flows/j3_strategy_editor_enter_autoindent.rs"]
mod j3_strategy_editor_enter_autoindent;
#[path = "e2e/flows/j4_strategy_editor_bracket_autoclose.rs"]
mod j4_strategy_editor_bracket_autoclose;
#[path = "e2e/flows/j5_find_panel_open_close_navigate.rs"]
mod j5_find_panel_open_close_navigate;
#[path = "e2e/flows/j6_find_replace_current_and_all.rs"]
mod j6_find_replace_current_and_all;
#[path = "e2e/flows/j7_startup_panel_validation_blocks_run.rs"]
mod j7_startup_panel_validation_blocks_run;
#[path = "e2e/flows/j8_startup_panel_valid_run_command.rs"]
mod j8_startup_panel_valid_run_command;
#[path = "e2e/flows/j9_instruments_ref_fail_closed.rs"]
mod j9_instruments_ref_fail_closed;
#[path = "e2e/flows/j10_instruments_ref_readonly_sidebar.rs"]
mod j10_instruments_ref_readonly_sidebar;
#[path = "e2e/flows/j11_instrument_picker_search_add_close.rs"]
mod j11_instrument_picker_search_add_close;
#[path = "e2e/flows/j12_instrument_picker_placeholders.rs"]
mod j12_instrument_picker_placeholders;
#[path = "e2e/flows/j13_sidebar_instrument_select_remove.rs"]
mod j13_sidebar_instrument_select_remove;
#[path = "e2e/flows/j14_scenario_schema_normalization.rs"]
mod j14_scenario_schema_normalization;
#[path = "e2e/flows/j15_scenario_file_watch_reparse.rs"]
mod j15_scenario_file_watch_reparse;
#[path = "e2e/flows/j16_startup_panel_field_commit.rs"]
mod j16_startup_panel_field_commit;
#[path = "e2e/flows/j17_empty_fragment_not_merged.rs"]
mod j17_empty_fragment_not_merged;

// K. Chart interaction / Reconcile / Order UI
// k1: stub のみ（kind:render — ShapePainter + Text2d は GPU 必要、headless 不可）
#[path = "e2e/flows/k2_chart_wheel_zoom_clamps.rs"]
mod k2_chart_wheel_zoom_clamps;
#[path = "e2e/flows/k3_chart_drag_pan_and_double_click_reset.rs"]
mod k3_chart_drag_pan_and_double_click_reset;
#[path = "e2e/flows/k4_chart_ctrl_wheel_camera_zoom.rs"]
mod k4_chart_ctrl_wheel_camera_zoom;
#[path = "e2e/flows/k5_chart_ladder_live_mode.rs"]
mod k5_chart_ladder_live_mode;
#[path = "e2e/flows/k6_reconcile_modal_after_backend_restart.rs"]
mod k6_reconcile_modal_after_backend_restart;
#[path = "e2e/flows/k7_manual_order_submit_confirm.rs"]
mod k7_manual_order_submit_confirm;
#[path = "e2e/flows/k8_secret_modal_submit_retry.rs"]
mod k8_secret_modal_submit_retry;
#[path = "e2e/flows/k9_order_context_modify_cancel.rs"]
mod k9_order_context_modify_cancel;
#[path = "e2e/flows/k10_order_form_controls_and_validation.rs"]
mod k10_order_form_controls_and_validation;
#[path = "e2e/flows/k11_order_confirm_cancel_escape_priority.rs"]
mod k11_order_confirm_cancel_escape_priority;
#[path = "e2e/flows/k12_modify_modal_submit_cancel_validation.rs"]
mod k12_modify_modal_submit_cancel_validation;
#[path = "e2e/flows/k13_relogin_modal_dismiss_escape_priority.rs"]
mod k13_relogin_modal_dismiss_escape_priority;
#[path = "e2e/flows/k14_reconcile_modal_dismiss_escape_priority.rs"]
mod k14_reconcile_modal_dismiss_escape_priority;
#[path = "e2e/flows/k15_secret_modal_timeout_zeroize_empty_submit.rs"]
mod k15_secret_modal_timeout_zeroize_empty_submit;
#[path = "e2e/flows/k16_order_context_menu_open_close.rs"]
mod k16_order_context_menu_open_close;
#[path = "e2e/flows/k17_chart_resize_reflow.rs"]
mod k17_chart_resize_reflow;
#[path = "e2e/flows/k18_live_chart_resize_reflow.rs"]
mod k18_live_chart_resize_reflow;
#[path = "e2e/flows/k19_chart_size_persists.rs"]
mod k19_chart_size_persists;
#[path = "e2e/flows/k20_chart_size_sidecar_round_trip.rs"]
mod k20_chart_size_sidecar_round_trip;
#[path = "e2e/flows/k21_chart_size_map_cleared_on_despawn.rs"]
mod k21_chart_size_map_cleared_on_despawn;

// L. CLI / backend process / prod guard
// l1: stub のみ（PowerShell .ps1 — Windows 専用、darwin CI 不可）
#[path = "e2e/flows/l2_strategy_replay_cli_outputs_run_buffer.rs"]
mod l2_strategy_replay_cli_outputs_run_buffer;
#[path = "e2e/flows/l3_prod_guard_blocks_without_env.rs"]
mod l3_prod_guard_blocks_without_env;
// l4: stub のみ（kind:render — winit + GPU 必要、headless 不可）
#[path = "e2e/flows/l5_backend_process_launch_and_grpc_ready.rs"]
mod l5_backend_process_launch_and_grpc_ready;
#[path = "e2e/flows/l6_catalog_auto_build_from_jquants.rs"]
mod l6_catalog_auto_build_from_jquants;
#[path = "e2e/flows/l7_attach_live_venue_mismatch.rs"]
mod l7_attach_live_venue_mismatch;

// M. Window / sidebar (UI)
#[path = "e2e/flows/m1_sidebar_panel_buttons_spawn_windows.rs"]
mod m1_sidebar_panel_buttons_spawn_windows;
#[path = "e2e/flows/m2_window_drag_updates_position_and_autosave.rs"]
mod m2_window_drag_updates_position_and_autosave;
#[path = "e2e/flows/m3_window_close_hides_or_despawns.rs"]
mod m3_window_close_hides_or_despawns;
#[path = "e2e/flows/m4_window_focus_brings_to_front.rs"]
mod m4_window_focus_brings_to_front;
#[path = "e2e/flows/m5_panel_duplicate_policy.rs"]
mod m5_panel_duplicate_policy;
#[path = "e2e/flows/m7_startup_window_has_no_close_button.rs"]
mod m7_startup_window_has_no_close_button;
#[path = "e2e/flows/m8_startup_window_visibility_owned_by_mode.rs"]
mod m8_startup_window_visibility_owned_by_mode;
#[path = "e2e/flows/m9_startup_window_position_persists_visible_not_authoritative.rs"]
mod m9_startup_window_position_persists_visible_not_authoritative;
#[path = "e2e/flows/m10_window_resize_updates_size_and_autosave.rs"]
mod m10_window_resize_updates_size_and_autosave;
#[path = "e2e/flows/m11_startup_window_content_hides_with_panel.rs"]
mod m11_startup_window_content_hides_with_panel;
#[path = "e2e/flows/m12_strategy_editor_hidden_in_manual.rs"]
mod m12_strategy_editor_hidden_in_manual;
#[path = "e2e/flows/m13_run_result_no_close_button.rs"]
mod m13_run_result_no_close_button;
#[path = "e2e/flows/m14_run_result_mode_visibility.rs"]
mod m14_run_result_mode_visibility;
#[path = "e2e/flows/m15_run_result_button_absent.rs"]
mod m15_run_result_button_absent;
#[path = "e2e/flows/m16_run_result_visibility_follows_backend_mode.rs"]
mod m16_run_result_visibility_follows_backend_mode;
#[path = "e2e/flows/m17_issue41_realapp_smoke.rs"]
mod m17_issue41_realapp_smoke;
#[path = "e2e/flows/m18_footer_mode_visibility_follows_backend_mode.rs"]
mod m18_footer_mode_visibility_follows_backend_mode;
#[path = "e2e/flows/m19_strategy_editor_mode_visibility_follows_backend_mode.rs"]
mod m19_strategy_editor_mode_visibility_follows_backend_mode;
#[path = "e2e/flows/m20_mode_visibility_systems_run_after_status_update.rs"]
mod m20_mode_visibility_systems_run_after_status_update;
#[path = "e2e/flows/m21_floating_window_interactive_sprites_have_pickable.rs"]
mod m21_floating_window_interactive_sprites_have_pickable;
#[path = "e2e/flows/m22_run_result_stats_pnl_fallback.rs"]
mod m22_run_result_stats_pnl_fallback;
#[path = "e2e/flows/m23_run_result_stats_blank_in_replay.rs"]
mod m23_run_result_stats_blank_in_replay;
#[path = "e2e/flows/m24_help_settings_spawns_floating_window.rs"]
mod m24_help_settings_spawns_floating_window;
#[path = "e2e/flows/m25_run_result_startup_progress.rs"]
mod m25_run_result_startup_progress;

// N. Live Auto strategy execution (Phase 10: lifecycle / telemetry / safety / log)
#[path = "e2e/flows/n1_live_strategy_event_lifecycle.rs"]
mod n1_live_strategy_event_lifecycle;
#[path = "e2e/flows/n2_live_strategy_telemetry.rs"]
mod n2_live_strategy_telemetry;
#[path = "e2e/flows/n3_safety_rail_violation_toast.rs"]
mod n3_safety_rail_violation_toast;
#[path = "e2e/flows/n4_strategy_log_message_buffer.rs"]
mod n4_strategy_log_message_buffer;
#[path = "e2e/flows/n5_footer_play_starts_live_auto.rs"]
mod n5_footer_play_starts_live_auto;
#[path = "e2e/flows/n6_footer_play_starts_live_auto_via_real_footer.rs"]
mod n6_footer_play_starts_live_auto_via_real_footer;
#[path = "e2e/flows/n7_footer_play_blocked_writes_run_result.rs"]
mod n7_footer_play_blocked_writes_run_result;
#[path = "e2e/flows/n8_live_reject_surfaces_run_failed.rs"]
mod n8_live_reject_surfaces_run_failed;
#[path = "e2e/flows/n9_second_live_run_accepted_after_stopped.rs"]
mod n9_second_live_run_accepted_after_stopped;
#[path = "e2e/flows/n10_live_error_status_maps_to_failed.rs"]
mod n10_live_error_status_maps_to_failed;
#[path = "e2e/flows/n12_failed_status_preserves_rich_error.rs"]
mod n12_failed_status_preserves_rich_error;
#[path = "e2e/flows/n13_footer_live_auto_pause_resume.rs"]
mod n13_footer_live_auto_pause_resume;

// O. Live venue integration (TACHIBANA / kabusapi 統合フロー)
#[path = "e2e/flows/o1_tachibana_live_manual_add_subscribe.rs"]
mod o1_tachibana_live_manual_add_subscribe;

// Q. Rendering / platform (headless smoke)
#[path = "e2e/flows/q1_draw_closure_no_i32_overflow.rs"]
mod q1_draw_closure_no_i32_overflow;
