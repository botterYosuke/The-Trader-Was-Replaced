//! N1 live_strategy_event_lifecycle — Live Auto run の lifecycle が LiveRuns に反映されること。
//!
//! `LiveStrategyEvent` push が `LiveRuns` に run を upsert する。初回イベントで row が
//! 挿入され `started_ts_ms` が固定される。同 run_id の後続イベントは status / updated_ts_ms
//! だけ更新し、空の strategy_id は既知の値を消さない。
//! 詳細は `tests/e2e/FLOWS.md` の N1 を参照。

use crate::support::Harness;
use backcast::trading::BackendEvent;

#[test]
fn n1_live_strategy_event_lifecycle() {
    let mut h = Harness::new();
    assert!(h.live_runs().runs.is_empty());

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 1000,
    });

    let runs = h.live_runs().runs;
    assert_eq!(runs.len(), 1);
    let r = &runs[0];
    assert_eq!(r.run_id, "r-1");
    assert_eq!(r.strategy_id, "LIVE-001");
    assert_eq!(r.status, "RUNNING");
    assert_eq!(r.started_ts_ms, 1000);
    assert_eq!(r.updated_ts_ms, 1000);

    // Same run, later event with empty strategy_id: status/updated advance, the
    // known strategy_id and the original start time are preserved (no duplicate row).
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: String::new(),
        status: "PAUSED".to_string(),
        ts_ms: 2000,
    });

    let runs = h.live_runs().runs;
    assert_eq!(runs.len(), 1, "same run_id must not insert a duplicate");
    let r = &runs[0];
    assert_eq!(r.status, "PAUSED");
    assert_eq!(r.strategy_id, "LIVE-001", "empty strategy_id must not clear a known one");
    assert_eq!(r.started_ts_ms, 1000, "start time is fixed at the first event");
    assert_eq!(r.updated_ts_ms, 2000);
}
