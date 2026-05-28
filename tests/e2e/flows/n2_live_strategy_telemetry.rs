//! N2 live_strategy_telemetry — Live Run の PnL / order / fill カウンタが CurrentRun に乗ること。
//!
//! `LiveStrategyTelemetry` push が `CurrentRun` の run-scoped カウンタを更新する。lifecycle
//! イベントより先に届いても（`run_id` が None でも）カウンタが書き込まれる。後続の lifecycle
//! イベントは `state` だけ立て、telemetry カウンタを消さない。
//! 詳細は `tests/e2e/FLOWS.md` の N2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, RunState};

#[test]
fn n2_live_strategy_telemetry() {
    let mut h = Harness::new();

    // Telemetry can race ahead of the first lifecycle event.
    // CurrentRun.run_id stays None (telemetry does not set it), but counters are updated.
    h.send_event(BackendEvent::LiveStrategyTelemetry {
        run_id: "r-2".to_string(),
        strategy_id: "LIVE-002".to_string(),
        realized_pnl: 100.0,
        unrealized_pnl: -50.0,
        order_count: 3,
        fill_count: 2,
        ts_ms: 500,
    });

    let cr = h.current_run();
    assert_eq!(cr.run_id, None, "telemetry alone does not set run_id");
    assert_eq!(cr.strategy_name, "LIVE-002");
    assert_eq!(cr.state, RunState::Idle, "telemetry carries no lifecycle state");
    assert_eq!(cr.realized_pnl, 100.0);
    assert_eq!(cr.unrealized_pnl, -50.0);
    assert_eq!(cr.order_count, 3);
    assert_eq!(cr.fill_count, 2);

    // A later lifecycle event sets state but must not clobber the counters.
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-2".to_string(),
        strategy_id: String::new(),
        status: "RUNNING".to_string(),
        ts_ms: 600,
    });

    let cr = h.current_run();
    assert_eq!(cr.run_id, Some("r-2".to_string()));
    assert_eq!(cr.state, RunState::Running);
    assert_eq!(cr.order_count, 3, "lifecycle event must not reset telemetry");
    assert_eq!(cr.fill_count, 2);
    assert_eq!(cr.realized_pnl, 100.0);
}
