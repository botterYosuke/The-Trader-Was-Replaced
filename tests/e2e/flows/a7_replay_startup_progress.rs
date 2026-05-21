//! A7 replay_startup_progress — Run 押下で起動ウィンドウが開き、リプレイ起動の4段階が
//! 進捗ウィンドウに反映されること。
//!
//! 実 Run ボタンを本番経路で駆動すると `handle_strategy_run_system` が起動ウィンドウを
//! 開き（visible）startup_id を割り当てる。続いて backend が同じ startup_id で押し出す
//! `ReplayStartup` の4 stage（ResettingReplay → LoadingData → StartingStrategy →
//! WaitingForFirstTick）が順に `ReplayStartupProgress.phase` を駆動し、最終 stage で
//! `start_engine_accepted` が立つことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A7 を参照。

use crate::support::Harness;
use backcast::replay::ReplayStartupPhase;
use backcast::trading::{BackendStartupStage, BackendStatusUpdate, TransportCommand};

#[test]
fn a7_replay_startup_progress() {
    let mut h = Harness::new();
    let startup_id = h.run_via_ui();

    // Run 押下で RunStrategy が出て起動ウィンドウが開く。
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::RunStrategy { startup_id: id, .. } if *id == startup_id
        )),
        "Run は startup_id 付きの RunStrategy を発射するはず (got {cmds:?})"
    );
    assert!(
        h.startup_progress().visible,
        "Run 押下で起動進捗ウィンドウが開くはず"
    );

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id,
        stage: BackendStartupStage::ResettingReplay,
    });
    assert_eq!(
        h.startup_progress().phase,
        ReplayStartupPhase::ResettingReplay
    );

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id,
        stage: BackendStartupStage::LoadingData,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::LoadingData);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id,
        stage: BackendStartupStage::StartingStrategy,
    });
    assert_eq!(
        h.startup_progress().phase,
        ReplayStartupPhase::StartingStrategy
    );

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id,
        stage: BackendStartupStage::WaitingForFirstTick,
    });
    let p = h.startup_progress();
    assert_eq!(p.phase, ReplayStartupPhase::WaitingForFirstTick);
    assert!(p.start_engine_accepted);
}
