//! N2 live_strategy_telemetry — Live Run の PnL / order / fill カウンタが LiveRuns に乗ること。
//!
//! `LiveStrategyTelemetry` push が `LiveRuns` の run-scoped カウンタを更新する。lifecycle
//! イベントより先に届いても row を作る（status は空のまま）。後続の lifecycle イベントは
//! status だけ立て、telemetry カウンタを消さない。
//! 詳細は `tests/e2e/FLOWS.md` の N2 を参照。

use crate::support::Harness;
use backcast::trading::BackendEvent;

#[test]
fn n2_live_strategy_telemetry() {
    let mut h = Harness::new();

    // Telemetry can race ahead of the first lifecycle event — it still creates a row.
    h.send_event(BackendEvent::LiveStrategyTelemetry {
        run_id: "r-2".to_string(),
        strategy_id: "LIVE-002".to_string(),
        realized_pnl: 100.0,
        unrealized_pnl: -50.0,
        order_count: 3,
        fill_count: 2,
        ts_ms: 500,
    });

    let runs = h.live_runs().runs;
    assert_eq!(runs.len(), 1);
    let r = &runs[0];
    assert_eq!(r.run_id, "r-2");
    assert_eq!(r.strategy_id, "LIVE-002");
    assert!(r.status.is_empty(), "telemetry carries no lifecycle status");
    assert_eq!(r.realized_pnl, 100.0);
    assert_eq!(r.unrealized_pnl, -50.0);
    assert_eq!(r.order_count, 3);
    assert_eq!(r.fill_count, 2);

    // A later lifecycle event sets status but must not clobber the counters.
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-2".to_string(),
        strategy_id: String::new(),
        status: "RUNNING".to_string(),
        ts_ms: 600,
    });

    let runs = h.live_runs().runs;
    assert_eq!(runs.len(), 1, "lifecycle event must merge, not duplicate");
    let r = &runs[0];
    assert_eq!(r.status, "RUNNING");
    assert_eq!(r.order_count, 3, "lifecycle event must not reset telemetry");
    assert_eq!(r.fill_count, 2);
    assert_eq!(r.realized_pnl, 100.0);
}
