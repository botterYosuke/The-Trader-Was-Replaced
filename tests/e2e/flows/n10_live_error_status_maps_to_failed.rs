//! N10 live_error_status_maps_to_failed — バックエンドの "ERROR" ステータスが
//! `RunState::Failed` にマップされること。
//!
//! Bug: `LiveStrategyEvent { status: "ERROR" }` に対するアームが無く、
//! catch-all `_ => current_run.state.clone()` に落ちて state が変わらない。
//! 詳細は `tests/e2e/FLOWS.md` の N10 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, RunState};

#[test]
fn n10_live_error_status_maps_to_failed() {
    let mut h = Harness::new();

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-err".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 1000,
    });
    assert_eq!(h.current_run().state, RunState::Running);

    // Backend sends ERROR (e.g. strategy runtime exception)
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-err".to_string(),
        strategy_id: String::new(),
        status: "ERROR".to_string(),
        ts_ms: 2000,
    });

    assert!(
        matches!(h.current_run().state, RunState::Failed { .. }),
        "ERROR status must map to RunState::Failed, got: {:?}",
        h.current_run().state,
    );
}

#[test]
fn n10_live_error_on_first_event_maps_to_failed() {
    // ERROR can arrive even as the first lifecycle event (e.g. immediate launch failure)
    let mut h = Harness::new();

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-err2".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "ERROR".to_string(),
        ts_ms: 1000,
    });

    assert!(
        matches!(h.current_run().state, RunState::Failed { .. }),
        "ERROR as first event must map to RunState::Failed, got: {:?}",
        h.current_run().state,
    );
    assert_eq!(
        h.current_run().run_id,
        Some("r-err2".to_string()),
        "run_id must be set even on ERROR"
    );
}
