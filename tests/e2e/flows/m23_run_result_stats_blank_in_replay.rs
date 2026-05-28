//! M23 run_result_stats_blank_in_replay — Replay 実行中（`strategy_name` が空）に
//! Stats 行と Pnl 行が空白を返すことを保証する（kind:ui）。
//!
//! Bug: `RunState::Running | RunState::Paused` アームが無条件に
//! `"strat: {strategy_name}  o:{order_count} f:{fill_count}"` をフォーマットするため、
//! Replay run 中（LiveStrategyEvent が発火しないので `strategy_name` が空のまま）に
//! `"strat:   o:0 f:0"` というノイズが出る。
//! 詳細は `tests/e2e/FLOWS.md` の M23 を参照。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::replay::ReplayStartupProgress;
use backcast::trading::{CurrentRun, RunState};
use backcast::ui::run_result_panel::{RunResultLabel, run_result_panel_system, spawn_run_result_panel};

#[test]
fn m23_run_result_stats_blank_in_replay() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<CurrentRun>();
    app.init_resource::<ReplayStartupProgress>();
    app.add_systems(Update, run_result_panel_system);
    app.add_systems(Startup, |mut commands: Commands| {
        spawn_run_result_panel(&mut commands);
    });

    // spawn panels
    app.update();

    // Replay running: strategy_name is empty (LiveStrategyEvent never fires in Replay)
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.state = RunState::Running;
        cr.strategy_name = String::new();
        cr.order_count = 0;
        cr.fill_count = 0;
        cr.realized_pnl = 0.0;
        cr.unrealized_pnl = 0.0;
    }
    app.update();

    let (mut stats_text, mut pnl_text) = (String::new(), String::new());
    for (label, text) in app
        .world_mut()
        .query::<(&RunResultLabel, &Text2d)>()
        .iter(app.world())
    {
        match label {
            RunResultLabel::Stats => stats_text = text.0.clone(),
            RunResultLabel::Pnl => pnl_text = text.0.clone(),
            _ => {}
        }
    }

    assert!(
        stats_text.is_empty(),
        "Replay Running + empty strategy_name → Stats must be empty. Got: {:?}",
        stats_text
    );
    assert!(
        pnl_text.is_empty(),
        "Replay Running + pnl=0 → Pnl must be empty. Got: {:?}",
        pnl_text
    );

    // Paused during Replay: same — no noise
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.state = RunState::Paused;
    }
    app.update();

    let mut stats_paused = String::new();
    for (label, text) in app
        .world_mut()
        .query::<(&RunResultLabel, &Text2d)>()
        .iter(app.world())
    {
        if matches!(label, RunResultLabel::Stats) {
            stats_paused = text.0.clone();
        }
    }
    assert!(
        stats_paused.is_empty(),
        "Replay Paused + empty strategy_name → Stats must be empty. Got: {:?}",
        stats_paused
    );

    // Live Auto running with strategy_name set → Stats must show
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.state = RunState::Running;
        cr.strategy_name = "LIVE-001".to_string();
        cr.order_count = 3;
        cr.fill_count = 2;
        cr.realized_pnl = 1500.0;
        cr.unrealized_pnl = -200.0;
    }
    app.update();

    let (mut stats_live, mut pnl_live) = (String::new(), String::new());
    for (label, text) in app
        .world_mut()
        .query::<(&RunResultLabel, &Text2d)>()
        .iter(app.world())
    {
        match label {
            RunResultLabel::Stats => stats_live = text.0.clone(),
            RunResultLabel::Pnl => pnl_live = text.0.clone(),
            _ => {}
        }
    }

    assert!(
        stats_live.contains("LIVE-001"),
        "Running + non-empty strategy_name → Stats must show it. Got: {:?}",
        stats_live
    );
    assert!(
        stats_live.contains("o:3"),
        "Running + order_count=3 → Stats must show count. Got: {:?}",
        stats_live
    );
    assert!(
        pnl_live.contains("1500"),
        "Running + realized_pnl=1500 → Pnl must show value. Got: {:?}",
        pnl_live
    );
}
