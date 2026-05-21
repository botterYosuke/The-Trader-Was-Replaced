//! E2E flow runner — one file per flow under `tests/e2e/flows/`, all compiled
//! into this single test binary so `cargo test --test e2e_replay` runs them
//! together (separate `tests/*.rs` files would each become their own binary and
//! link the whole crate). See `tests/e2e/FLOWS.md` for the flow catalog.
//!
//! Only `.rs` files directly under `tests/` form a test crate, so the shared
//! harness and the per-flow files are pulled in via `#[path]` module
//! declarations. Each flow file holds exactly one `#[test]` — the file count
//! equals the test count (currently 38).

#[path = "e2e/support/mod.rs"]
mod support;

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

// K. Reconcile
#[path = "e2e/flows/k6_reconcile_modal_after_backend_restart.rs"]
mod k6_reconcile_modal_after_backend_restart;
