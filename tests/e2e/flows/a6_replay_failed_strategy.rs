//! A6 replay_failed_strategy — 壊れた戦略で実行すると失敗が UI に出ること。
//!
//! `RunStarted` → `RunFailed{error}` で `RunState::Failed{error}` に遷移し、
//! backend が返した error 文字列がそのまま surface されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A6 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn a6_replay_failed_strategy() {
    let mut h = Harness::new();

    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunFailed {
        startup_id: None,
        error: "boom: strategy import error".to_string(),
    });

    match h.run_state() {
        RunState::Failed { error } => {
            assert!(error.contains("boom"), "error not surfaced: {error}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
