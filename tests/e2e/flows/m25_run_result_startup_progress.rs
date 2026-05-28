//! M25 run_result_startup_progress — `ReplayStartupProgress.visible` が true のとき
//! RUN RESULT パネルがフェーズラベルとインジケータバーを表示し、通常行を隠す（kind:ui）。
//!
//! 起動完了後（visible=false）は通常行が "No run yet" を表示し、
//! 起動セクションが非表示になることも検証する。

use backcast::replay::{ReplayStartupPhase, ReplayStartupProgress};
use backcast::trading::{CurrentRun, ExecutionModeRes};
use backcast::ui::components::{RunResultPanelRoot, WindowManager};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::run_result_panel::{
    RunResultBarBg, RunResultLabel, RunResultPhaseLabel,
    run_result_panel_system, spawn_run_result_panel,
};
use bevy::prelude::*;
use bevy::transform::TransformPlugin;

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.insert_resource(ReplayStartupProgress::default());
    app.insert_resource(CurrentRun::default());
    app.insert_resource(ExecutionModeRes::default());
    app.insert_resource(WindowManager::default());
    app.insert_resource(AppHistory::default());
    app.add_systems(Startup, |mut commands: Commands| {
        spawn_run_result_panel(&mut commands);
    });
    app.add_systems(Update, run_result_panel_system);
    app
}

#[test]
fn m25_startup_progress_shows_phase_label() {
    let mut app = make_app();
    app.update(); // Startup spawn

    assert!(
        app.world_mut()
            .query_filtered::<Entity, With<RunResultPanelRoot>>()
            .iter(app.world())
            .next()
            .is_some(),
        "RUN RESULT root が spawn されているはず"
    );

    {
        let mut p = app.world_mut().resource_mut::<ReplayStartupProgress>();
        p.visible = true;
        p.phase = ReplayStartupPhase::LoadingData;
        p.error = None;
    }
    app.update();

    let world = app.world_mut();

    let mut phase_vis_q = world.query_filtered::<&Visibility, With<RunResultPhaseLabel>>();
    let phase_vis = phase_vis_q.single(world).unwrap();
    assert_eq!(*phase_vis, Visibility::Inherited, "起動中はフェーズラベルが表示のはず");

    let mut phase_text_q = world.query_filtered::<&Text2d, With<RunResultPhaseLabel>>();
    let phase_text = phase_text_q.single(world).unwrap();
    assert!(
        phase_text.0.contains("Loading"),
        "LoadingData フェーズのラベルに 'Loading' が含まれるはず: got {:?}",
        phase_text.0
    );

    let mut bar_q = world.query_filtered::<&Visibility, With<RunResultBarBg>>();
    let bar_vis = bar_q.single(world).unwrap();
    assert_eq!(*bar_vis, Visibility::Inherited, "起動中はインジケータバーが表示のはず");

    let mut row_q = world.query_filtered::<&Text2d, With<RunResultLabel>>();
    for text in row_q.iter(world) {
        assert!(
            text.0.is_empty(),
            "起動中は通常行のテキストが空のはず: got {:?}",
            text.0
        );
    }
}

#[test]
fn m25_startup_done_shows_normal_rows() {
    let mut app = make_app();
    app.update(); // Startup spawn

    {
        let mut p = app.world_mut().resource_mut::<ReplayStartupProgress>();
        p.visible = false;
        p.phase = ReplayStartupPhase::Idle;
    }
    app.update();

    let world = app.world_mut();

    let mut phase_vis_q = world.query_filtered::<&Visibility, With<RunResultPhaseLabel>>();
    let phase_vis = phase_vis_q.single(world).unwrap();
    assert_eq!(*phase_vis, Visibility::Hidden, "起動完了後はフェーズラベルが非表示のはず");

    let mut bar_q = world.query_filtered::<&Visibility, With<RunResultBarBg>>();
    let bar_vis = bar_q.single(world).unwrap();
    assert_eq!(*bar_vis, Visibility::Hidden, "起動完了後はバーが非表示のはず");

    let mut row_q = world.query_filtered::<&Text2d, With<RunResultLabel>>();
    let texts: Vec<String> = row_q.iter(world).map(|t| t.0.clone()).collect();
    assert!(
        texts.iter().any(|t| t == "No run yet"),
        "起動完了後は State 行が 'No run yet' のはず: got {:?}",
        texts
    );
}
