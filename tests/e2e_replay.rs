//! E2E replay-lifecycle flows (v1) driving the backend → ECS sync layer through
//! a headless Bevy app. See `tests/e2e/FLOWS.md` for the full flow catalog.
//!
//! Only `.rs` files directly under `tests/` form a test crate, so the shared
//! harness under `tests/e2e/support/` is pulled in via `#[path]`.

#[path = "e2e/support/mod.rs"]
mod support;

use support::Harness;

use backcast::replay::ReplayStartupPhase;
use backcast::trading::{
    AccountPosition, BackendEvent, BackendStartupStage, BackendStatusUpdate, ExecutionMode,
    PortfolioOrder, PortfolioPosition, RunState, Ticker, TickersSource, TickersStatus, VenueState,
};
use chrono::NaiveDate;

/// A1 replay_runs_to_completion: RunStarted → Running, RunComplete fills summary.
#[test]
fn a1_replay_runs_to_completion() {
    let mut h = Harness::new();
    assert_eq!(h.run_state(), RunState::Idle);

    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-1".to_string(),
        summary_json: r#"{"fills_count":3,"equity_points":10,"total_pnl":1234.5,"status":"ok"}"#
            .to_string(),
    });

    assert_eq!(h.run_state(), RunState::Completed);
    let last = h.last_run();
    assert_eq!(last.run_id.as_deref(), Some("run-1"));
    assert!(last.summary_json.is_some());
    let summary = last.parsed_summary.expect("summary parsed");
    assert_eq!(summary.fills_count, 3);
    assert_eq!(summary.total_pnl, 1234.5);
}

/// A6 replay_failed_strategy: RunStarted → RunFailed → Failed{error}, error surfaced.
#[test]
fn a6_replay_failed_strategy() {
    let mut h = Harness::new();

    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunFailed {
        startup_id: None,
        error: "boom: strategy import error".to_string(),
    });

    match h.run_state() {
        RunState::Failed { error } => {
            assert!(error.contains("boom"), "error not surfaced: {error}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// B1 portfolio_populated_after_run: PortfolioLoaded fills positions/orders/equity.
#[test]
fn b1_portfolio_populated_after_run() {
    let mut h = Harness::new();
    assert!(!h.portfolio().loaded);

    h.send_status(BackendStatusUpdate::PortfolioLoaded {
        buying_power: 100_000.0,
        cash: 50_000.0,
        equity: 150_000.0,
        positions: vec![PortfolioPosition {
            symbol: "1301.TSE".to_string(),
            qty: 100,
            avg_price: 1500.0,
            unrealized_pnl: 250.0,
        }],
        orders: vec![PortfolioOrder {
            symbol: "1301.TSE".to_string(),
            side: "BUY".to_string(),
            qty: 100.0,
            price: 1500.0,
            status: "FILLED".to_string(),
            ts_ms: 1000,
        }],
    });

    let p = h.portfolio();
    assert!(p.loaded);
    assert_eq!(p.equity, 150_000.0);
    assert_eq!(p.buying_power, 100_000.0);
    assert_eq!(p.positions.len(), 1);
    assert_eq!(p.positions[0].symbol, "1301.TSE");
    assert_eq!(p.orders.len(), 1);
    assert_eq!(p.orders[0].status, "FILLED");
}

/// A2 replay_pause_resume: UI mirrors the backend-pushed replay clock. While
/// no new state is pushed (pause), `timestamp_ms` stays put; resuming advances it.
/// Real Pause/Resume semantics over gRPC depend on the transport task (Phase A-full);
/// v1 verifies the UI faithfully mirrors the clock the backend pushes.
#[test]
fn a2_replay_pause_resume() {
    let mut h = Harness::new();

    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.push_state(1000);
    assert_eq!(h.timestamp_ms(), 1000);

    // Pause: no new state pushed, ticking must not advance the clock.
    h.tick();
    h.tick();
    assert_eq!(h.timestamp_ms(), 1000);

    // Resume: backend pushes a later clock, UI mirrors it.
    h.push_state(2000);
    assert_eq!(h.timestamp_ms(), 2000);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-2".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}

/// A3 replay_step_forward: each backend-pushed clock state advances
/// `TradingSession.timestamp_ms` by one step. StepForward itself is a
/// `TransportCommand` (gRPC); v1 verifies the UI mirrors the per-step clock
/// the backend pushes back.
#[test]
fn a3_replay_step_forward() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    for step in 1..=4i64 {
        h.push_state(step);
        assert_eq!(h.timestamp_ms(), step);
    }
}

/// A4 replay_force_stop: ForceStop ends the run; on the backend → ECS seam this
/// surfaces as `RunComplete`, which moves `RunState` out of `Running`.
#[test]
fn a4_replay_force_stop() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-forced".to_string(),
        summary_json: r#"{"status":"stopped"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}

/// A7 replay_startup_progress: the four `ReplayStartup` stages drive
/// `ReplayStartupProgress.phase`, and the final stage flips
/// `start_engine_accepted`.
#[test]
fn a7_replay_startup_progress() {
    let mut h = Harness::new();
    h.begin_startup(7);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::ResettingReplay,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::ResettingReplay);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::LoadingData,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::LoadingData);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::StartingStrategy,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::StartingStrategy);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::WaitingForFirstTick,
    });
    let p = h.startup_progress();
    assert_eq!(p.phase, ReplayStartupPhase::WaitingForFirstTick);
    assert!(p.start_engine_accepted);
}

/// A8 stale_startup_id_ignored: a `RunComplete` carrying an old `startup_id`
/// must not close the current startup window; only the matching id closes it.
/// Regression guard for the correlation-id logic.
#[test]
fn a8_stale_startup_id_ignored() {
    let mut h = Harness::new();
    h.begin_startup(9);

    // Stale completion from a superseded run — window must stay open.
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(8),
        run_id: "stale".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert!(h.startup_progress().visible, "stale id closed the window");

    // Matching completion closes the window.
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(9),
        run_id: "current".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert!(!h.startup_progress().visible, "matching id did not close window");
}

/// A9 replay_startup_timeout: an armed startup window that never receives a
/// first tick flips to a soft-timeout error after 60s (and the error persists
/// until the user closes the window). Drives the production
/// `replay_startup_timeout_system` via an advanced headless `Time<Real>`.
#[test]
fn a9_replay_startup_timeout() {
    let mut h = Harness::new();
    h.arm_startup_timeout(42);

    // Just under the threshold: no error yet.
    h.advance_real_time(std::time::Duration::from_secs(59));
    assert!(h.startup_progress().error.is_none(), "must not time out before 60s");
    assert!(h.startup_progress().visible);

    // Past the threshold: soft-timeout error surfaces, window stays open.
    h.advance_real_time(std::time::Duration::from_secs(2));
    let p = h.startup_progress();
    assert!(p.error.is_some(), "soft timeout must set an error after 60s");
    assert!(p.visible, "window stays open so the user can read the error");
    assert_eq!(
        p.phase,
        ReplayStartupPhase::WaitingForFirstTick,
        "timeout annotates with error but does not change the phase"
    );

    // Close dismisses the error and hides the window.
    h.close_startup_window();
    let p = h.startup_progress();
    assert!(!p.visible);
    assert!(p.error.is_none());
}

/// B2 run_summary_parsed: `RunComplete{summary_json}` populates
/// `LastRunResult.parsed_summary` (fills_count / equity_points / total_pnl).
#[test]
fn b2_run_summary_parsed() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-summary".to_string(),
        summary_json: r#"{"fills_count":7,"equity_points":42,"total_pnl":-1500.25,"status":"ok"}"#
            .to_string(),
    });

    let summary = h.last_run().parsed_summary.expect("summary parsed");
    assert_eq!(summary.fills_count, 7);
    assert_eq!(summary.equity_points, 42);
    assert_eq!(summary.total_pnl, -1500.25);
    assert_eq!(summary.status, "ok");
}

/// C1 list_instruments_replay: InstrumentsListStarted → Loaded fills `Tickers`
/// and records the source.
#[test]
fn c1_list_instruments_replay() {
    let mut h = Harness::new();
    assert_eq!(h.tickers().status, TickersStatus::NotFetched);

    h.send_status(BackendStatusUpdate::InstrumentsListStarted {
        source: TickersSource::ReplayCatalogFallback,
    });
    assert_eq!(h.tickers().status, TickersStatus::InFlight);

    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![
            Ticker { id: "1301.TSE".into(), name: "Kyokuyo".into(), market: "TSE".into() },
            Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() },
        ],
    });
    let t = h.tickers();
    assert_eq!(t.status, TickersStatus::Loaded);
    assert_eq!(t.source, TickersSource::ReplayCatalogFallback);
    assert_eq!(t.list.len(), 2);
}

/// C2 list_instruments_failed: a failed fetch sets `Failed` status while keeping
/// the previously loaded list (stale display).
#[test]
fn c2_list_instruments_failed() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![Ticker {
            id: "7203.TSE".into(),
            name: "Toyota".into(),
            market: "TSE".into(),
        }],
    });

    h.send_status(BackendStatusUpdate::InstrumentsListFailed {
        source: TickersSource::LiveVenue,
        error: "grpc timeout".to_string(),
    });

    let t = h.tickers();
    assert_eq!(t.status, TickersStatus::Failed("grpc timeout".to_string()));
    assert_eq!(t.source, TickersSource::LiveVenue);
    assert_eq!(t.list.len(), 1, "stale list must be retained");
    assert_eq!(t.list[0].id, "7203.TSE");
}

/// C3 fetch_available_instruments: AvailableInstrumentsLoaded fills
/// `by_end_date[end_date]` and clears the `in_flight` marker.
#[test]
fn c3_fetch_available_instruments() {
    let mut h = Harness::new();
    let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();

    h.send_status(BackendStatusUpdate::AvailableInstrumentsLoaded {
        end_date,
        ids: vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
    });

    let a = h.available();
    assert_eq!(a.by_end_date.get(&end_date).map(|v| v.len()), Some(2));
    assert!(!a.in_flight.contains(&end_date));
    assert!(a.last_error.is_none());
}

/// C4 fetch_available_failed: a failed fetch records `last_error` with the
/// end_date and clears `in_flight`.
#[test]
fn c4_fetch_available_failed() {
    let mut h = Harness::new();
    let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();

    h.send_status(BackendStatusUpdate::AvailableInstrumentsFetchFailed {
        end_date,
        error: "no catalog".to_string(),
    });

    let a = h.available();
    let (date, err) = a.last_error.expect("last_error set");
    assert_eq!(date, end_date);
    assert_eq!(err, "no catalog");
    assert!(!a.in_flight.contains(&end_date));
}

/// D1 venue_login_success: VenueChanged drives the venue lifecycle
/// Disconnected → Authenticating → Connected.
#[test]
fn d1_venue_login_success() {
    let mut h = Harness::new();
    assert_eq!(h.venue().state, VenueState::Disconnected);

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Authenticating,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Authenticating);

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    let v = h.venue();
    assert_eq!(v.state, VenueState::Connected);
    assert_eq!(v.venue_id.as_deref(), Some("tachibana"));
}

/// D2 venue_subscribed: Connected → Subscribed reflects `instruments_loaded`.
#[test]
fn d2_venue_subscribed() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Subscribed,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 128,
    });
    let v = h.venue();
    assert_eq!(v.state, VenueState::Subscribed);
    assert_eq!(v.instruments_loaded, 128);
}

/// D3 venue_login_error: a failed login surfaces as `VenueState::Error`.
#[test]
fn d3_venue_login_error() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Error,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Error);
}

/// D4 venue_logout: logout returns the venue to Disconnected.
#[test]
fn d4_venue_logout() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });
    assert_eq!(h.venue().state, VenueState::Connected);

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Disconnected);
}

/// D6 venue_reconnecting: a network drop surfaces as `VenueState::Reconnecting`.
#[test]
fn d6_venue_reconnecting() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Reconnecting,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });
    assert_eq!(h.venue().state, VenueState::Reconnecting);
}

/// D7 live_universe_overwrite: a Live-venue list overwrites a prior Replay
/// fallback list wholesale (overwrite, not union — see plan §0.5.1).
#[test]
fn d7_live_universe_overwrite() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![
            Ticker { id: "1301.TSE".into(), name: "Kyokuyo".into(), market: "TSE".into() },
            Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() },
        ],
    });

    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::LiveVenue,
        instruments: vec![Ticker {
            id: "9984.TSE".into(),
            name: "SoftBank".into(),
            market: "TSE".into(),
        }],
    });

    let t = h.tickers();
    assert_eq!(t.source, TickersSource::LiveVenue);
    assert_eq!(t.list.len(), 1, "live universe must overwrite the fallback list");
    assert_eq!(t.list[0].id, "9984.TSE");
    assert!(
        !t.list.iter().any(|x| x.id == "1301.TSE"),
        "fallback entries must not survive the overwrite"
    );
}

/// E1 set_execution_mode: backend-authoritative `ExecutionModeChanged` drives
/// `ExecutionModeRes.mode`.
#[test]
fn e1_set_execution_mode() {
    let mut h = Harness::new();
    assert_eq!(h.exec_mode().mode, ExecutionMode::Replay);

    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(h.exec_mode().mode, ExecutionMode::LiveManual);
}

/// F1 subscribe_market_data: `LastPricesUpdated` fills `LastPrices.map`.
#[test]
fn f1_subscribe_market_data() {
    let mut h = Harness::new();
    assert!(h.last_prices().map.is_empty());

    let mut prices = std::collections::HashMap::new();
    prices.insert("7203.TSE".to_string(), 2500.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });

    assert_eq!(h.last_prices().map.get("7203.TSE"), Some(&2500.0));
}

/// F2 unsubscribe_market_data: a subsequent wholesale `LastPricesUpdated`
/// without the instrument clears its price (Replay/unsubscribe emits an empty
/// or reduced map).
#[test]
fn f2_unsubscribe_market_data() {
    let mut h = Harness::new();
    let mut prices = std::collections::HashMap::new();
    prices.insert("7203.TSE".to_string(), 2500.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });
    assert!(h.last_prices().map.contains_key("7203.TSE"));

    h.send_status(BackendStatusUpdate::LastPricesUpdated {
        prices: std::collections::HashMap::new(),
    });
    assert!(h.last_prices().map.is_empty(), "unsubscribe clears the price map");
}

/// G1 backend_connect_status: Connected/Running status updates set the
/// `BackendStatus` flags (footer `grpc: OK`).
#[test]
fn g1_backend_connect_status() {
    let mut h = Harness::new();
    assert!(!h.backend_connected());
    assert!(!h.backend_running());

    h.send_status(BackendStatusUpdate::Connected(true));
    h.send_status(BackendStatusUpdate::Running(true));

    assert!(h.backend_connected());
    assert!(h.backend_running());
}

/// G2 backend_reconnect_selfheal: an `Error` drops the connection (and records
/// the error), and a later `Connected(true)` brings it back. Regression guard
/// for the self-heal path — connection state must recover, not latch off.
#[test]
fn g2_backend_reconnect_selfheal() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::Connected(true));
    assert!(h.backend_connected());

    // Connection drops.
    h.send_status(BackendStatusUpdate::Error("stream reset".to_string()));
    assert!(!h.backend_connected(), "Error must drop the connection");
    assert_eq!(h.backend_last_error().as_deref(), Some("stream reset"));

    // Recovery: a fresh Connected(true) re-establishes the link.
    h.send_status(BackendStatusUpdate::Connected(true));
    assert!(h.backend_connected(), "connection must self-heal, not latch off");
}

/// G3 backend_disabled_sim: with `backend_enabled = false` the footer renders
/// `grpc: DISABLED` and `backend_update_system` early-returns, so a pushed
/// replay-clock state is a no-op (the session clock never advances).
#[test]
fn g3_backend_disabled_sim() {
    let mut h = Harness::new_backend_disabled();
    assert!(!h.backend_enabled(), "footer would render grpc: DISABLED");
    assert_eq!(h.timestamp_ms(), 0);

    // backend_update_system early-returns: the pushed clock must be ignored.
    h.push_state(5000);
    assert_eq!(h.timestamp_ms(), 0, "disabled backend must not advance the clock");
}

/// F3 order_event: a `BackendEvent::OrderEvent` for an unknown `client_order_id`
/// inserts a `LiveOrders` record (empty static fields) carrying the event's
/// status/fill, and a later event for the same id merges in place.
#[test]
fn f3_order_event() {
    let mut h = Harness::new();
    assert!(h.live_orders().orders.is_empty());

    h.send_event(BackendEvent::OrderEvent {
        order_id: "o-1".to_string(),
        venue_order_id: "v-1".to_string(),
        client_order_id: "c-1".to_string(),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
        strategy_id: String::new(),
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(o.client_order_id, "c-1");
    assert_eq!(o.venue_order_id, "v-1");
    assert_eq!(o.status, "WORKING");
    // Unknown id inserted with empty static fields (no PlaceOrder seed yet).
    assert!(o.symbol.is_empty());
    assert_eq!(o.filled_qty, 0.0);

    // A fill update for the same client_order_id merges in place.
    h.send_event(BackendEvent::OrderEvent {
        order_id: "o-1".to_string(),
        venue_order_id: "v-1".to_string(),
        client_order_id: "c-1".to_string(),
        status: "FILLED".to_string(),
        filled_qty: 100.0,
        avg_price: 1500.0,
        ts_ms: 2000,
        strategy_id: String::new(),
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1, "merge must not insert a duplicate");
    let o = &orders[0];
    assert_eq!(o.status, "FILLED");
    assert_eq!(o.filled_qty, 100.0);
    assert_eq!(o.avg_price, 1500.0);
}

/// F4 account_event: a `BackendEvent::AccountEvent` populates `PortfolioState`
/// (cash/buying_power/positions, loaded), with equity derived as
/// `cash + Σ(qty*avg_price + unrealized_pnl)`.
#[test]
fn f4_account_event() {
    let mut h = Harness::new();
    assert!(!h.portfolio().loaded);

    h.send_event(BackendEvent::AccountEvent {
        cash: 50_000.0,
        buying_power: 120_000.0,
        positions: vec![
            AccountPosition {
                symbol: "1301.TSE".to_string(),
                qty: 100,
                avg_price: 1500.0,
                unrealized_pnl: 250.0,
            },
            AccountPosition {
                symbol: "7203.TSE".to_string(),
                qty: 10,
                avg_price: 2000.0,
                unrealized_pnl: -100.0,
            },
        ],
        ts_ms: 1000,
    });

    let p = h.portfolio();
    assert!(p.loaded);
    assert_eq!(p.cash, 50_000.0);
    assert_eq!(p.buying_power, 120_000.0);
    assert_eq!(p.positions.len(), 2);
    assert_eq!(p.positions[0].symbol, "1301.TSE");
    // cash + (100*1500 + 250) + (10*2000 + -100) = 50000 + 150250 + 19900
    assert_eq!(p.equity, 50_000.0 + 150_250.0 + 19_900.0);
}

/// F5 secret_required: a `BackendEvent::SecretRequired` opens the secret prompt
/// (`SecretPrompt.active` becomes `Some` with the request's fields).
#[test]
fn f5_secret_required() {
    let mut h = Harness::new();
    assert!(h.secret_prompt().active.is_none());

    h.send_event(BackendEvent::SecretRequired {
        request_id: "req-1".to_string(),
        venue: "tachibana".to_string(),
        kind: "second_password".to_string(),
        purpose: "place_order".to_string(),
    });

    let req = h.secret_prompt().active.expect("prompt active");
    assert_eq!(req.request_id, "req-1");
    assert_eq!(req.venue, "tachibana");
    assert_eq!(req.kind, "second_password");
    assert_eq!(req.purpose, "place_order");
}

/// D5 venue_logout_detected: a `BackendEvent::VenueLogoutDetected` opens the
/// relogin prompt (`ReloginPrompt.active` becomes `Some(venue)`) so the user is
/// told the venue dropped. Implemented in production by the Phase 9 Step 7
/// health-watchdog merge; the modal only notifies (re-login goes through the
/// Venue menu), so `VenueStatusRes` is intentionally left unchanged here.
#[test]
fn d5_venue_logout_detected() {
    let mut h = Harness::new();
    assert!(h.relogin_prompt().active.is_none());

    h.send_event(BackendEvent::VenueLogoutDetected {
        venue: "KABU".to_string(),
    });

    assert_eq!(h.relogin_prompt().active.as_deref(), Some("KABU"));
}

/// H1 order_seeded: `OrderSeeded` seeds the full `LiveOrders` record (including
/// the static symbol/side/qty/price the OrderEvent lacks) and clears any prior
/// `OrderFeedback` notice.
#[test]
fn h1_order_seeded() {
    let mut h = Harness::new();
    // A stale reject notice from a prior attempt.
    h.send_status(BackendStatusUpdate::OrderRejected {
        action: "発注".to_string(),
        error_code: "VENUE_LOGIN_REQUIRED".to_string(),
    });
    assert!(h.order_feedback().message.is_some());

    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        symbol: "1301.TSE".to_string(),
        side: "BUY".to_string(),
        qty: 100.0,
        price: Some(1500.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(o.client_order_id, "c-1");
    assert_eq!(o.symbol, "1301.TSE");
    assert_eq!(o.side, "BUY");
    assert_eq!(o.qty, 100.0);
    assert_eq!(o.price, Some(1500.0));
    assert!(
        h.order_feedback().message.is_none(),
        "a successful seed clears the prior reject notice"
    );
}

/// H2 order_status_updated: `OrderStatusUpdated` merges status/fill into the
/// existing seeded record by `client_order_id`, preserving static fields.
#[test]
fn h2_order_status_updated() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        symbol: "1301.TSE".to_string(),
        side: "BUY".to_string(),
        qty: 100.0,
        price: Some(1500.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
    });

    h.send_status(BackendStatusUpdate::OrderStatusUpdated {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        status: "FILLED".to_string(),
        filled_qty: 100.0,
        avg_price: 1499.0,
        ts_ms: 2000,
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1, "merge must not insert a duplicate");
    let o = &orders[0];
    assert_eq!(o.status, "FILLED");
    assert_eq!(o.filled_qty, 100.0);
    assert_eq!(o.avg_price, 1499.0);
    // Static fields from the seed survive the merge.
    assert_eq!(o.symbol, "1301.TSE");
    assert_eq!(o.qty, 100.0);
}

/// H3 order_modified: `OrderModified` overwrites qty/price only when `Some`
/// (None keeps the tracked value) and refreshes status/fills.
#[test]
fn h3_order_modified() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        symbol: "1301.TSE".to_string(),
        side: "BUY".to_string(),
        qty: 100.0,
        price: Some(1500.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
    });

    // Modify only the price; qty is None so it must stay at 100.
    h.send_status(BackendStatusUpdate::OrderModified {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        new_qty: None,
        new_price: Some(1450.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 2000,
    });

    let o = h.live_orders().orders[0].clone();
    assert_eq!(o.price, Some(1450.0), "Some(price) overwrites");
    assert_eq!(o.qty, 100.0, "None new_qty keeps the tracked value");
    assert_eq!(o.ts_ms, 2000);
}

/// H4 order_rejected: `OrderRejected` surfaces a formatted notice in
/// `OrderFeedback.message`.
#[test]
fn h4_order_rejected() {
    let mut h = Harness::new();
    assert!(h.order_feedback().message.is_none());

    h.send_status(BackendStatusUpdate::OrderRejected {
        action: "発注".to_string(),
        error_code: "EXECUTION_MODE_PRECONDITION".to_string(),
    });

    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("発注が拒否されました (EXECUTION_MODE_PRECONDITION)")
    );
}

/// H5 exec_mode_change_resets_portfolio: a real `ExecutionModeChanged` resets
/// `PortfolioState` to default so Live/Replay account data cannot bleed across
/// modes. A no-op change (same mode) leaves the portfolio intact.
#[test]
fn h5_exec_mode_change_resets_portfolio() {
    let mut h = Harness::new();
    // Populate Live account data.
    h.send_event(BackendEvent::AccountEvent {
        cash: 50_000.0,
        buying_power: 120_000.0,
        positions: vec![AccountPosition {
            symbol: "1301.TSE".to_string(),
            qty: 100,
            avg_price: 1500.0,
            unrealized_pnl: 0.0,
        }],
        ts_ms: 1000,
    });
    assert!(h.portfolio().loaded);

    // A no-op change (already Replay) must not wipe the data.
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });
    assert!(h.portfolio().loaded, "same-mode change is a no-op for the portfolio");

    // A real change wipes the mode-specific account snapshot.
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    let p = h.portfolio();
    assert!(!p.loaded, "a real mode change resets the portfolio");
    assert!(p.positions.is_empty());
    assert_eq!(p.cash, 0.0);
}
