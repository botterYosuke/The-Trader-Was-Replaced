//! N9 second_live_run_accepted_after_stopped — 1回目の Live Auto run が終了後、
//! 2回目の run_id を持つ `LiveStrategyEvent` が `CurrentRun` に受け入れられること。
//!
//! Bug: `run_id` は一度セットされると None に戻らないため、`run_id.is_none()` だけを
//! 新規受け入れ条件にすると 1 回目の run 終了後に 2 回目の run が無視される。
//! 詳細は `tests/e2e/FLOWS.md` の N9 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, RunState};

#[test]
fn n9_second_live_run_accepted_after_stopped() {
    let mut h = Harness::new();

    // First run: RUNNING → STOPPED
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 1000,
    });
    assert_eq!(h.current_run().run_id, Some("r-1".to_string()));
    assert_eq!(h.current_run().state, RunState::Running);

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: String::new(),
        status: "STOPPED".to_string(),
        ts_ms: 2000,
    });
    assert_eq!(h.current_run().state, RunState::Stopped);

    // 2nd run: must be accepted even though run_id is already set to "r-1"
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-2".to_string(),
        strategy_id: "LIVE-002".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 3000,
    });

    let cr = h.current_run();
    assert_eq!(
        cr.run_id,
        Some("r-2".to_string()),
        "2nd run_id must update after STOPPED"
    );
    assert_eq!(cr.state, RunState::Running, "2nd run must be Running");
    assert_eq!(
        cr.strategy_name, "LIVE-002",
        "strategy_name must reset to 2nd run"
    );
    assert_eq!(
        cr.started_ts_ms, 3000,
        "started_ts_ms must reset to 2nd run start"
    );
    assert_eq!(cr.order_count, 0, "order_count must reset for new run");
    assert_eq!(cr.fill_count, 0, "fill_count must reset for new run");

    // Telemetry for the 2nd run must also be accepted
    h.send_event(BackendEvent::LiveStrategyTelemetry {
        run_id: "r-2".to_string(),
        strategy_id: String::new(),
        realized_pnl: 500.0,
        unrealized_pnl: -100.0,
        order_count: 2,
        fill_count: 1,
        ts_ms: 3500,
    });
    let cr = h.current_run();
    assert_eq!(cr.order_count, 2, "telemetry for 2nd run must be accepted");
    assert_eq!(cr.realized_pnl, 500.0, "realized_pnl for 2nd run must update");
}

#[test]
fn n9_second_live_run_accepted_after_failed() {
    let mut h = Harness::new();

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-a".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 1000,
    });
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-a".to_string(),
        strategy_id: String::new(),
        status: "FAILED".to_string(),
        ts_ms: 2000,
    });
    assert!(matches!(h.current_run().state, RunState::Failed { .. }));

    // 2nd run after Failed must also be accepted
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-b".to_string(),
        strategy_id: "LIVE-002".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 3000,
    });

    assert_eq!(
        h.current_run().run_id,
        Some("r-b".to_string()),
        "new run after Failed must update run_id"
    );
    assert_eq!(h.current_run().state, RunState::Running);
}
