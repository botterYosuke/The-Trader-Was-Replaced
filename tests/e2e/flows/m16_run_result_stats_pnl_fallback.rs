//! M16 run_result_stats_pnl_fallback — Live Auto run が STOPPED/FAILED になったとき、
//! `parsed_summary` が None でも `order_count`/`fill_count` > 0 なら
//! Stats 行が live カウントを表示し続けることを保証する（kind:ui）。
//!
//! #42 Slice 2 で `run_result_panel_system` に追加したフォールバック arm の回帰ガード。
//! このアームが消えると Live Auto run 停止後に Stats/Pnl が空白になる。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{CurrentRun, RunState};
use backcast::ui::run_result_panel::{RunResultLabel, run_result_panel_system, spawn_run_result_panel};

#[test]
fn m16_run_result_stats_pnl_fallback() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<CurrentRun>();
    app.add_systems(Update, run_result_panel_system);
    app.add_systems(Startup, |mut commands: Commands| {
        spawn_run_result_panel(&mut commands);
    });

    // spawn panels
    app.update();

    // STOPPED with live counts — fallback arm should show counts
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.state = RunState::Stopped;
        cr.strategy_name = "my_strat".to_string();
        cr.order_count = 5;
        cr.fill_count = 3;
        cr.realized_pnl = 1200.0;
        cr.unrealized_pnl = -300.0;
    }
    app.update();

    let mut stats_text = String::new();
    let mut pnl_text = String::new();
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
        stats_text.contains("o:5"),
        "STOPPED + order_count>0 → Stats should show order count. Got: {:?}",
        stats_text
    );
    assert!(
        stats_text.contains("f:3"),
        "STOPPED + fill_count>0 → Stats should show fill count. Got: {:?}",
        stats_text
    );
    assert!(
        pnl_text.contains("1200"),
        "STOPPED + realized_pnl≠0 → Pnl should show realized. Got: {:?}",
        pnl_text
    );

    // FAILED with live counts — same fallback
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.state = RunState::Failed {
            error: "gRPC rejected".to_string(),
        };
    }
    app.update();

    let mut stats_failed = String::new();
    for (label, text) in app
        .world_mut()
        .query::<(&RunResultLabel, &Text2d)>()
        .iter(app.world())
    {
        if matches!(label, RunResultLabel::Stats) {
            stats_failed = text.0.clone();
        }
    }

    assert!(
        stats_failed.contains("o:5"),
        "FAILED + order_count>0 → Stats fallback should still show order count. Got: {:?}",
        stats_failed
    );

    // counts zero — Stats should be empty (no noise when there is nothing to show)
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.state = RunState::Stopped;
        cr.order_count = 0;
        cr.fill_count = 0;
        cr.realized_pnl = 0.0;
        cr.unrealized_pnl = 0.0;
    }
    app.update();

    let mut stats_empty = String::new();
    for (label, text) in app
        .world_mut()
        .query::<(&RunResultLabel, &Text2d)>()
        .iter(app.world())
    {
        if matches!(label, RunResultLabel::Stats) {
            stats_empty = text.0.clone();
        }
    }

    assert!(
        stats_empty.is_empty(),
        "STOPPED + all counts zero → Stats should be empty. Got: {:?}",
        stats_empty
    );
}
