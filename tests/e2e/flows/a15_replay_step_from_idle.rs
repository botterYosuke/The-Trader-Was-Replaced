//! A15 replay_step_from_idle — IDLE 状態で ▶| (StepForward) を押すと
//! "load then step" コマンドが transport channel に送られること。
//!
//! PAUSED 中の StepForward（A3 でカバー済み）とは異なり、IDLE から押した場合は
//! 策略フラッシュ → LoadReplayData → StepReplay という順序が必要になる。
//! `transport_button_system` が IDLE を無視する現在の実装を回帰ガードする。
//! 詳細は `tests/e2e/FLOWS.md` の A15 を参照。

use crate::support::Harness;
use backcast::trading::{ExecutionMode, ExecutionModeRes, TransportCommand};
use backcast::ui::components::{
    InstrumentRegistry, ScenarioMetadata, StrategyBuffer, TransportButton,
};

#[test]
fn a15_replay_step_from_idle() {
    let mut h = Harness::new();
    let dir = tempfile::TempDir::new().unwrap();

    // Replay mode + 最小有効シナリオ（handle_strategy_run_system が通過できる最小セット）
    h.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    {
        let mut sc = h.app.world_mut().resource_mut::<ScenarioMetadata>();
        sc.instruments = vec!["7203.TSE".to_string()];
        sc.start = Some("2025-01-06".to_string());
        sc.end = Some("2025-03-31".to_string());
        sc.granularity = Some("Daily".to_string());
        sc.initial_cash = Some(1_000_000);
    }
    h.app
        .world_mut()
        .resource_mut::<InstrumentRegistry>()
        .editable = false;

    // cache_path を事前設定（flush_strategy_cache が Ok(true) を返す条件）
    let cache_py = dir.path().join("cache.py");
    std::fs::write(&cache_py, "x = 1\n").unwrap();
    h.app
        .world_mut()
        .resource_mut::<StrategyBuffer>()
        .cache_path = Some(cache_py);

    h.set_replay_state(None); // IDLE

    h.click(TransportButton::StepForward);
    h.tick();

    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::LoadAndStep { .. })),
        "IDLE から StepForward は LoadAndStep を送るはず (got {:?})", cmds
    );
}
