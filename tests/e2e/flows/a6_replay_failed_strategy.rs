//! A6 replay_failed_strategy — Run した戦略が壊れていると失敗が UI に出ること。
//!
//! 実 Run ボタンを本番経路で駆動して RunStrategy を送った後、backend が
//! `RunFailed{error}` を status seam に押し戻すと `RunState::Failed{error}` に遷移し、
//! backend が返した error 文字列がそのまま surface されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A6 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn a6_replay_failed_strategy() {
    let mut h = Harness::new();
    let startup_id = h.run_via_ui();
    h.drain_commands();
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunFailed {
        startup_id: Some(startup_id),
        error: "boom: strategy import error".to_string(),
    });

    match h.run_state() {
        RunState::Failed { error } => {
            assert!(error.contains("boom"), "error not surfaced: {error}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
