//! M13 run_result_no_close_button — 本番の `spawn_run_result_panel`
//! が生成する RUN RESULT ウィンドウには `×` クローズボタンが存在しないことを保証する（kind:ui）。
//!
//! `closeable: false` で `spawn_floating_window` を呼ぶため CloseButton entity は spawn されない。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{CloseButton, RunResultPanelRoot, WindowManager};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::run_result_panel::spawn_run_result_panel;

#[test]
fn m13_run_result_no_close_button() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.insert_resource(WindowManager::default());
    app.insert_resource(AppHistory::default());

    app.add_systems(Startup, |mut commands: Commands| {
        spawn_run_result_panel(&mut commands);
    });
    app.update();

    // RUN RESULT ウィンドウ root が spawn されていること（前提確認）
    let root_count = app
        .world_mut()
        .query_filtered::<Entity, With<RunResultPanelRoot>>()
        .iter(app.world())
        .count();
    assert_eq!(root_count, 1, "RUN RESULT ウィンドウ root が 1 個 spawn されるはず");

    // × クローズボタンが 1 個も spawn されていないこと
    let close_count = app
        .world_mut()
        .query_filtered::<Entity, With<CloseButton>>()
        .iter(app.world())
        .count();
    assert_eq!(
        close_count, 0,
        "RUN RESULT ウィンドウは closeable:false なので CloseButton を spawn しないはず"
    );
}
