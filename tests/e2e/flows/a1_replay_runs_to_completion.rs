//! A1 replay_runs_to_completion — リプレイが Run ボタン押下から完了まで走り切るフロー。
//!
//! 実ユーザー操作（フッターの Run ボタン）を本番経路で駆動する: footer の
//! `footer_pause_resume_system` が `StrategyRunRequested` を発火 →
//! `handle_strategy_run_system` が `TransportCommand::RunStrategy{startup_id}` を
//! transport channel に送り `RunState` を Running にする。続いて backend が
//! `RunComplete{summary_json}` を status seam に押し戻すと Completed になり、
//! summary JSON が `LastRunResult.parsed_summary` にパースされることまで確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState, TransportCommand};

#[test]
fn a1_replay_runs_to_completion() {
    let mut h = Harness::new();
    assert_eq!(h.run_state(), RunState::Idle);

    // 実 Run ボタン押下 → 本番 run-request チェーン → RunStrategy コマンド送信。
    let startup_id = h.run_via_ui();
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::RunStrategy { startup_id: id, .. } if *id == startup_id
        )),
        "Run ボタンは割り当てた startup_id を載せた RunStrategy を発射するはず (got {cmds:?})"
    );
    assert_eq!(
        h.run_state(),
        RunState::Running,
        "run 要求の時点で RunState は Running になる"
    );

    // backend が run ライフサイクルを status seam に押し戻す。
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(startup_id),
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
