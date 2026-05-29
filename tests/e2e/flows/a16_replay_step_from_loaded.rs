//! A16 replay_step_from_loaded — LOADED 状態で ▶| (StepForward) を押すと
//! `TransportCommand::StepForward` が送出されること（LoadAndStep ではない）。
//!
//! issue #64 Phase 0 回帰ガード。LOADED は既に catalog を読み込んでいるので
//! LoadReplayData を再実行せず即時 StepReplay を呼ぶ経路のみを使う。
//! fix 前は else ブランチで無視されるため、このテストは RED になる。

use crate::support::Harness;
use backcast::trading::TransportCommand;
use backcast::ui::components::TransportButton;

#[test]
fn a16_replay_step_from_loaded() {
    let mut h = Harness::new();
    h.run_via_ui();
    h.drain_commands();

    h.set_replay_state(Some("LOADED"));
    h.click(TransportButton::StepForward);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::StepForward)),
        "LOADED 中の StepForward 押下は StepForward を送るはず (got {cmds:?})"
    );
}
