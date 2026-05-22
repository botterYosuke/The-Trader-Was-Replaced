//! M8 startup_window_visibility_owned_by_mode — 本番の Startup ウィンドウ root の
//! `Visibility` が `ExecutionMode` によって切り替わることを保証する（kind:ui）。
//! Replay → 可視（`Inherited`）、Manual / Auto → 非可視（`Hidden`）。
//!
//! `apply_startup_panel_visibility_system` を実 window root（`ScenarioStartupPanelRoot`）に対して
//! 駆動する。可視性は `WindowLayout.visible` ではなく実行モードが所有する（[M9] と対）。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes};
use backcast::ui::components::ScenarioStartupPanelRoot;
use backcast::ui::scenario_startup_panel::{
    apply_startup_panel_visibility_system, spawn_scenario_startup_window,
};

#[test]
fn m8_startup_window_visibility_owned_by_mode() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<ExecutionModeRes>(); // 既定 = Replay
    app.add_systems(Update, apply_startup_panel_visibility_system);

    {
        let mut commands = app.world_mut().commands();
        spawn_scenario_startup_window(&mut commands);
    }
    app.world_mut().flush();

    let root = app
        .world_mut()
        .query_filtered::<Entity, With<ScenarioStartupPanelRoot>>()
        .iter(app.world())
        .next()
        .expect("Startup ウィンドウ root が存在するはず");

    // ── Replay（既定）→ 可視 ──
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "Replay モードでは Startup ウィンドウは可視のはず"
    );

    // ── Manual → 非可視 ──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Hidden,
        "Manual モードでは Startup ウィンドウは非可視のはず"
    );

    // ── Replay へ戻す → 再び可視 ──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "Replay へ戻すと Startup ウィンドウは再び可視のはず"
    );
}
