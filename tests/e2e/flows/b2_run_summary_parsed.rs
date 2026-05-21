//! B2 run_summary_parsed — Run 実行サマリ JSON がパースされること。
//!
//! 実 Run ボタンを本番経路で駆動した後、backend が `RunComplete{summary_json}` を
//! 押し戻すと、その JSON が `LastRunResult.parsed_summary`（fills_count /
//! equity_points / total_pnl / status）に正しくパースされることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の B2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn b2_run_summary_parsed() {
    let mut h = Harness::new();
    let startup_id = h.run_via_ui();
    h.drain_commands();
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(startup_id),
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
