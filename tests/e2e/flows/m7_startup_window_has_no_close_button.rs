//! M7 startup_window_has_no_close_button — 本番の `spawn_scenario_startup_window`
//! が生成する Startup ウィンドウには `×` クローズボタンが存在しないことを保証する（kind:ui）。
//!
//! Startup は `closeable: false` で `spawn_floating_window` を呼ぶ唯一のウィンドウ。
//! `:118` を `true` に戻すと（= 他の窓に合わせて × を付けると）ユーザーが
//! replay 実行条件を編集する唯一の窓を閉じられてしまう（ADR-0001）。その回帰ガード。
//! 比較対照として closeable な窓は M3 が `CloseButton` 1 個を確認している。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{CloseButton, ScenarioStartupPanelRoot};
use backcast::ui::scenario_startup_panel::spawn_scenario_startup_window;

#[test]
fn m7_startup_window_has_no_close_button() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    {
        let mut commands = app.world_mut().commands();
        spawn_scenario_startup_window(&mut commands);
    }
    app.world_mut().flush();
    app.update();

    // Startup ウィンドウ root が spawn されていること（前提確認）。
    let root_count = app
        .world_mut()
        .query_filtered::<Entity, With<ScenarioStartupPanelRoot>>()
        .iter(app.world())
        .count();
    assert_eq!(root_count, 1, "Startup ウィンドウ root が 1 個 spawn されるはず");

    // × クローズボタンが 1 個も spawn されていないこと。
    let close_count = app
        .world_mut()
        .query_filtered::<Entity, With<CloseButton>>()
        .iter(app.world())
        .count();
    assert_eq!(
        close_count, 0,
        "Startup ウィンドウは closeable:false なので CloseButton を spawn しないはず"
    );
}
