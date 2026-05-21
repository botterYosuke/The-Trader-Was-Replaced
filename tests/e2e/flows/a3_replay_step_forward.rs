//! A3 replay_step_forward — Pause 中に StepForward ボタンが StepForward コマンドを送り、
//! backend が押し戻す per-step 時計を UI が1単位ずつミラーすること。
//!
//! 実フッターの ▶| ボタンを本番 `transport_button_system` で駆動する。replay 状態が
//! PAUSED のときだけ `TransportCommand::StepForward` を送ることを transport channel で
//! 観測し、続く per-step state push ごとに `TradingSession.timestamp_ms` が進むことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A3 を参照。

use crate::support::Harness;
use backcast::trading::{RunState, TransportCommand};
use backcast::ui::components::TransportButton;

#[test]
fn a3_replay_step_forward() {
    let mut h = Harness::new();
    h.run_via_ui();
    h.drain_commands();
    assert_eq!(h.run_state(), RunState::Running);

    // ステップ実行は Pause 中の操作。
    h.set_replay_state(Some("PAUSED"));
    h.click(TransportButton::StepForward);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::StepForward)),
        "PAUSED 中の StepForward 押下は StepForward を送るはず (got {cmds:?})"
    );

    // backend が1ステップごとに押し出す時計を UI が忠実にミラーする。
    for step in 1..=4i64 {
        h.push_state(step);
        assert_eq!(h.timestamp_ms(), step);
    }
}
