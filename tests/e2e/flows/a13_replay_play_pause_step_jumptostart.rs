//! A13 replay_play_pause_step_jumptostart — ▶️ クリック → Pause → StepForward → |< の
//! 一連ユーザーセッションを通しで検証する結合テスト。
//!
//! A2/A3/A4 は各操作を個別に単体テストするが、本テストはリプレイの典型的な
//! 操作フロー全体が正しい `TransportCommand` を順番に送出することを確認する。
//! step- (1 bar 戻る) は未実装のため、|< (JumpToStart) を代用する。
//! #58: JumpToStart は `ForceStop` ではなく `RestartReplay` を送出する（バー 0 リロード）。
//! 詳細は `tests/e2e/FLOWS.md` の A13 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState, TransportCommand};
use backcast::ui::components::{PauseResumeButton, TransportButton};

#[test]
fn a13_replay_play_pause_step_jumptostart() {
    let mut h = Harness::new();

    // ▶️ クリック: 実フッターシステム経由で RunStrategy を送出。
    h.run_via_ui();
    h.drain_commands();
    assert_eq!(h.run_state(), RunState::Running);

    // backend が RUNNING を返した想定にする。
    h.set_replay_state(Some("RUNNING"));

    // || を押す → RUNNING なので Pause コマンドが送られる。
    h.click(PauseResumeButton);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::Pause)),
        "RUNNING 中の PauseResume 押下は Pause を送るはず (got {cmds:?})"
    );

    // backend が PAUSED を返した想定にする。
    h.set_replay_state(Some("PAUSED"));

    // > を押す → PAUSED 中のみ有効。StepForward コマンドが送られる。
    h.click(TransportButton::StepForward);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::StepForward)),
        "PAUSED 中の StepForward 押下は StepForward を送るはず (got {cmds:?})"
    );

    // backend が 1 tick 進めた時計を返す。
    // push_state は replay_state を持たないためリセットされる — 直後に再セットする (A2 と同じパターン)。
    h.push_state(1);
    assert_eq!(
        h.timestamp_ms(),
        1,
        "step 後の時計は backend が返す値をミラーするはず"
    );
    h.set_replay_state(Some("PAUSED"));

    // |< を押す → PAUSED 中なので RestartReplay コマンドが送られる（#58: バー 0 リロード）。
    h.click(TransportButton::JumpToStart);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::RestartReplay { .. })),
        "PAUSED 中の JumpToStart 押下は RestartReplay を送るはず (got {cmds:?})"
    );

    // backend が完走を通知 → Completed に遷移する。
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-a13".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}
