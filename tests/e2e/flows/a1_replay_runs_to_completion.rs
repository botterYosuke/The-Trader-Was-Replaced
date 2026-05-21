//! A1 replay_runs_to_completion — リプレイが起動から完了まで走り切るフロー。
//!
//! backend が `RunStarted` を送ると `RunState` が Idle→Running に遷移し、
//! `RunComplete{summary_json}` 受信で Completed になる。さらに summary JSON
//! （fills_count / total_pnl 等）が `LastRunResult.parsed_summary` にパース
//! されることまで確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

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
