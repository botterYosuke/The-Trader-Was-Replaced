//! A4 replay_force_stop — Stop ボタンが ForceStop コマンドを送り、backend の
//! `RunComplete` でリプレイが Running を抜けること。
//!
//! 実フッターの ■ ボタンを本番 `transport_button_system` で駆動する。replay 状態が
//! RUNNING/PAUSED/LOADED のとき `TransportCommand::ForceStop` を送ることを transport
//! channel で観測する。ForceStop は backend→ECS seam では `RunComplete` として現れ、
//! 受信後に `RunState` が Completed になることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState, TransportCommand};
use backcast::ui::components::TransportButton;

#[test]
fn a4_replay_force_stop() {
    let mut h = Harness::new();
    h.run_via_ui();
    h.drain_commands();
    assert_eq!(h.run_state(), RunState::Running);

    // ユーザーが Stop ボタンを押す → RUNNING なので ForceStop コマンド。
    h.set_replay_state(Some("RUNNING"));
    h.click(TransportButton::ForceStop);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::ForceStop)),
        "RUNNING 中の Stop 押下は ForceStop を送るはず (got {cmds:?})"
    );

    // backend は ForceStop を RunComplete として押し戻す。
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-forced".to_string(),
        summary_json: r#"{"status":"stopped"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}
