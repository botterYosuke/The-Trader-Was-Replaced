//! A4 replay_force_stop — 強制停止でリプレイが Running から抜けること。
//!
//! ForceStop は backend→ECS seam では `RunComplete` として現れる。受信後に
//! `RunState` が Running を抜ける（Completed になる）ことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn a4_replay_force_stop() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-forced".to_string(),
        summary_json: r#"{"status":"stopped"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}
