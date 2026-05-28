//! N1 live_strategy_event_lifecycle — Live Auto run の lifecycle が CurrentRun に反映されること。
//!
//! `LiveStrategyEvent` push が `CurrentRun` を upsert する。初回イベントで `run_id` /
//! `strategy_name` / `started_ts_ms` がセットされる。同 run_id の後続イベントは `state` だけ
//! 更新し、空の `strategy_id` は既知の `strategy_name` を消さない。
//! 詳細は `tests/e2e/FLOWS.md` の N1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, RunState};

#[test]
fn n1_live_strategy_event_lifecycle() {
    let mut h = Harness::new();
    assert_eq!(h.current_run().run_id, None, "initial state: no run");

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 1000,
    });

    let cr = h.current_run();
    assert_eq!(cr.run_id, Some("r-1".to_string()));
    assert_eq!(cr.strategy_name, "LIVE-001");
    assert_eq!(cr.state, RunState::Running);
    assert_eq!(cr.started_ts_ms, 1000);

    // Same run, later event with empty strategy_id: state advances, the
    // known strategy_name and the original start time are preserved.
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: String::new(),
        status: "PAUSED".to_string(),
        ts_ms: 2000,
    });

    let cr = h.current_run();
    assert_eq!(cr.state, RunState::Paused);
    assert_eq!(cr.strategy_name, "LIVE-001", "empty strategy_id must not clear a known one");
    assert_eq!(cr.started_ts_ms, 1000, "start time is fixed at the first event");
}
