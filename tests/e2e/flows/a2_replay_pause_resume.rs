//! A2 replay_pause_resume — Run 後に Pause/Resume ボタンが正しいコマンドを送り、
//! 一時停止中はリプレイ時計が止まり再開で進むこと。
//!
//! 実フッター操作を本番 `footer_pause_resume_system` で駆動する。replay 状態に
//! 応じて同じ Pause/Resume ボタンが `TransportCommand::Pause` / `Resume` を送り
//! 分けることを transport channel で観測する。時計のミラーは backend が押し出す
//! state（pause 中は来ない）を `TradingSession.timestamp_ms` が忠実に追うことで確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState, TransportCommand};
use backcast::ui::components::PauseResumeButton;

#[test]
fn a2_replay_pause_resume() {
    let mut h = Harness::new();
    h.run_via_ui();
    h.drain_commands(); // clear the RunStrategy command from the Run click.
    assert_eq!(h.run_state(), RunState::Running);

    // backend pushes the replay clock, then confirms RUNNING. Order matters: a
    // clock push carries no replay_state and would reset it, so set it afterward.
    h.push_state(1000);
    assert_eq!(h.timestamp_ms(), 1000);
    h.set_replay_state(Some("RUNNING"));

    // ユーザーが Pause ボタンを押す → RUNNING なので Pause コマンド。
    h.click(PauseResumeButton);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::Pause)),
        "RUNNING 中の押下は Pause を送るはず (got {cmds:?})"
    );

    // Pause 中: 新しい state が来ないので tick しても時計は進まない。
    h.tick();
    h.tick();
    assert_eq!(h.timestamp_ms(), 1000);

    // ユーザーが Resume ボタンを押す → PAUSED なので Resume コマンド。
    h.set_replay_state(Some("PAUSED"));
    h.click(PauseResumeButton);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::Resume)),
        "PAUSED 中の押下は Resume を送るはず (got {cmds:?})"
    );

    // Resume: backend が後続の時計を押し出し UI がミラーする。
    h.push_state(2000);
    assert_eq!(h.timestamp_ms(), 2000);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-2".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}
