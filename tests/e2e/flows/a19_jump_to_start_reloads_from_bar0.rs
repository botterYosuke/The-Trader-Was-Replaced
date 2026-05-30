//! A19 jump_to_start_reloads_from_bar0 — `|<` が RUNNING/PAUSED/LOADED/IDLE から
//! 押されたとき `TransportCommand::RestartReplay` を送出すること。
//!
//! 旧実装: JumpToStart は RUNNING/PAUSED/LOADED のとき `ForceStop` だけを送り、
//!          IDLE は無視（"jump_to_start ignored"）。
//! 新実装: すべての replay 状態から `RestartReplay` を送る。
//!          backend 側で「RUNNING/PAUSED → ForceStop → LoadReplayData」
//!                     「IDLE/LOADED → LoadReplayData のみ」を実行し、
//!          `▶` でバー 0 から再生できる状態（LOADED）に戻す。
//!
//! seam: `transport_button_system` の `TransportButton::JumpToStart` arm
//! 観測: `drain_commands()` で `RestartReplay` が含まれること
//! 関連: [A4] ForceStop 単体 / [A13] ジャーニー / issue #58

use crate::support::Harness;
use backcast::trading::TransportCommand;
use backcast::ui::components::{ScenarioMetadata, TransportButton};

/// シナリオメタデータを有効な値で設定する（JumpToStart の config guard を通過させる）。
fn seed_scenario(h: &mut Harness) {
    let mut sc = h.app.world_mut().resource_mut::<ScenarioMetadata>();
    sc.instruments = vec!["7203.TSE".to_string()];
    sc.start = Some("2025-01-06".to_string());
    sc.end = Some("2025-03-31".to_string());
    sc.granularity = Some("Daily".to_string());
    sc.initial_cash = Some(1_000_000);
}

/// RUNNING 中の `|<` → RestartReplay を送出する
#[test]
fn a19_jump_to_start_from_running_sends_restart() {
    let mut h = Harness::new();
    seed_scenario(&mut h);
    h.set_replay_state(Some("RUNNING"));

    h.click(TransportButton::JumpToStart);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::RestartReplay { .. })),
        "RUNNING 中の |< は RestartReplay を送るはず (got {cmds:?})"
    );
}

/// PAUSED 中の `|<` → RestartReplay を送出する
#[test]
fn a19_jump_to_start_from_paused_sends_restart() {
    let mut h = Harness::new();
    seed_scenario(&mut h);
    h.set_replay_state(Some("PAUSED"));

    h.click(TransportButton::JumpToStart);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::RestartReplay { .. })),
        "PAUSED 中の |< は RestartReplay を送るはず (got {cmds:?})"
    );
}

/// LOADED 中の `|<` → RestartReplay を送出する
#[test]
fn a19_jump_to_start_from_loaded_sends_restart() {
    let mut h = Harness::new();
    seed_scenario(&mut h);
    h.set_replay_state(Some("LOADED"));

    h.click(TransportButton::JumpToStart);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::RestartReplay { .. })),
        "LOADED 中の |< は RestartReplay を送るはず (got {cmds:?})"
    );
}

/// IDLE 中の `|<` → RestartReplay を送出する（旧実装では無視されていた）
#[test]
fn a19_jump_to_start_from_idle_sends_restart() {
    let mut h = Harness::new();
    seed_scenario(&mut h);
    h.set_replay_state(None); // IDLE

    h.click(TransportButton::JumpToStart);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::RestartReplay { .. })),
        "IDLE からの |< は RestartReplay を送るはず（旧: ignored）(got {cmds:?})"
    );
}
