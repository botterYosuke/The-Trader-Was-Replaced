//! M14 run_result_mode_visibility — RUN RESULT ウィンドウの `Visibility` が
//! `ExecutionMode` によって切り替わることを保証する（kind:ui）。
//!
//! Replay / LiveAuto → `Inherited`、LiveManual → `Hidden`（issue #41 仕様）。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes};
use backcast::ui::components::{RunResultPanelRoot, WindowManager};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::run_result_panel::{apply_run_result_visibility_system, spawn_run_result_panel};

#[test]
fn m14_run_result_mode_visibility() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<ExecutionModeRes>();
    app.insert_resource(WindowManager::default());
    app.insert_resource(AppHistory::default());
    app.add_systems(Update, apply_run_result_visibility_system);

    app.add_systems(Startup, |mut commands: Commands| {
        spawn_run_result_panel(&mut commands);
    });
    app.update();

    let root = app
        .world_mut()
        .query_filtered::<Entity, With<RunResultPanelRoot>>()
        .iter(app.world())
        .next()
        .expect("RUN RESULT ウィンドウ root が存在するはず");

    // Replay（既定）→ 可視
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "Replay モードでは RUN RESULT は可視のはず"
    );

    // LiveManual → 非表示
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Hidden,
        "LiveManual モードでは RUN RESULT は非表示のはず"
    );

    // Replay へ戻す → 再び可視
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "Replay へ戻すと RUN RESULT は再び可視のはず"
    );

    // LiveAuto → 可視のまま
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "LiveAuto では RUN RESULT は可視のはず"
    );
}
