//! A7 replay_startup_progress — リプレイ起動の4段階が進捗ウィンドウに反映されること。
//!
//! `ReplayStartup` の4 stage（ResettingReplay → LoadingData → StartingStrategy
//! → WaitingForFirstTick）が順に `ReplayStartupProgress.phase` を駆動し、最終
//! stage で `start_engine_accepted` が立つことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A7 を参照。

use crate::support::Harness;
use backcast::replay::ReplayStartupPhase;
use backcast::trading::{BackendStartupStage, BackendStatusUpdate};

#[test]
fn a7_replay_startup_progress() {
    let mut h = Harness::new();
    h.begin_startup(7);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::ResettingReplay,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::ResettingReplay);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::LoadingData,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::LoadingData);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::StartingStrategy,
    });
    assert_eq!(h.startup_progress().phase, ReplayStartupPhase::StartingStrategy);

    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id: 7,
        stage: BackendStartupStage::WaitingForFirstTick,
    });
    let p = h.startup_progress();
    assert_eq!(p.phase, ReplayStartupPhase::WaitingForFirstTick);
    assert!(p.start_engine_accepted);
}
