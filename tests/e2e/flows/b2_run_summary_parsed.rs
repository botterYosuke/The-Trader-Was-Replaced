//! B2 run_summary_parsed — 実行サマリ JSON がパースされること。
//!
//! `RunComplete{summary_json}` の JSON が `LastRunResult.parsed_summary`
//! （fills_count / equity_points / total_pnl / status）に正しくパースされること
//! を確認する。
//! 詳細は `tests/e2e/FLOWS.md` の B2 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

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
