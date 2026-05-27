//! N12 failed_status_preserves_rich_error — `LiveStrategyEvent{status:"FAILED"}` が
//! `RunFailed` チャネル経由で書き込まれたリッチな error を上書きしないこと。
//!
//! Bug: `RunFailed` (status channel) が `Failed{error:"詳細メッセージ"}` を書いた後、
//! `backend_event_drain_system` が `LiveStrategyEvent{status:"FAILED"}` を処理して
//! `Failed{error:""}` で上書きする。UI には `Failed:  ` と空白が表示される。
//! 詳細は `tests/e2e/FLOWS.md` の N12 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, BackendStatusUpdate, RunState};

#[test]
fn n12_failed_status_preserves_rich_error() {
    let mut h = Harness::new();

    // status_update_system writes a rich error via RunFailed (e.g. gRPC reject from StartLiveStrategy)
    h.send_status(BackendStatusUpdate::RunFailed {
        startup_id: None,
        error: "gRPC rejected: RESOURCE_EXHAUSTED".to_string(),
    });

    assert!(
        matches!(&h.current_run().state, RunState::Failed { error } if error == "gRPC rejected: RESOURCE_EXHAUSTED"),
        "RunFailed must set Failed with rich error, got: {:?}",
        h.current_run().state,
    );

    // Backend then sends LiveStrategyEvent{status:"FAILED"} — must NOT overwrite the rich error
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-1".to_string(),
        strategy_id: String::new(),
        status: "FAILED".to_string(),
        ts_ms: 1000,
    });

    assert!(
        matches!(&h.current_run().state, RunState::Failed { error } if error == "gRPC rejected: RESOURCE_EXHAUSTED"),
        "LiveStrategyEvent FAILED must not overwrite a rich error. Got: {:?}",
        h.current_run().state,
    );
}

#[test]
fn n12_error_status_also_preserves_rich_error() {
    // Same invariant for status:"ERROR"
    let mut h = Harness::new();

    h.send_status(BackendStatusUpdate::RunFailed {
        startup_id: None,
        error: "SyntaxError: unexpected token".to_string(),
    });

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-2".to_string(),
        strategy_id: String::new(),
        status: "ERROR".to_string(),
        ts_ms: 1000,
    });

    assert!(
        matches!(&h.current_run().state, RunState::Failed { error } if error == "SyntaxError: unexpected token"),
        "LiveStrategyEvent ERROR must not overwrite a rich error. Got: {:?}",
        h.current_run().state,
    );
}

#[test]
fn n12_failed_with_empty_error_stays_failed() {
    // When there is no pre-existing rich error, FAILED sets Failed{error:""}
    let mut h = Harness::new();

    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-3".to_string(),
        strategy_id: "LIVE-001".to_string(),
        status: "RUNNING".to_string(),
        ts_ms: 500,
    });
    h.send_event(BackendEvent::LiveStrategyEvent {
        run_id: "r-3".to_string(),
        strategy_id: String::new(),
        status: "FAILED".to_string(),
        ts_ms: 1000,
    });

    assert!(
        matches!(&h.current_run().state, RunState::Failed { error } if error.is_empty()),
        "FAILED with no prior rich error must set Failed with empty error. Got: {:?}",
        h.current_run().state,
    );
}
