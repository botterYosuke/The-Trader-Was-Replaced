//! M15 run_result_button_absent — 本番の `spawn_sidebar` が生成するサイドバーに
//! `PanelKind::RunResult` のボタンが存在しないことを保証する（kind:ui）。
//!
//! RUN RESULT は ExecutionMode 所有（起動時自動 spawn）であり、
//! ユーザーが手動で開閉するサイドバーボタンは設けない（issue #41 仕様）。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{PanelKind, WindowManager};
use backcast::ui::sidebar::spawn_sidebar;

#[test]
fn m15_run_result_button_absent() {
    let mut app = App::new();
    app.init_resource::<backcast::ui::theme::Theme>();
    app.add_plugins(TransformPlugin);
    app.insert_resource(WindowManager::default());
    app.add_systems(Startup, spawn_sidebar);
    app.update();

    // PanelKind::RunResult かつ Button を持つ entity が 0 件であること
    let count = app
        .world_mut()
        .query_filtered::<Entity, (With<PanelKind>, With<Button>)>()
        .iter(app.world())
        .filter(|&e| {
            matches!(app.world().get::<PanelKind>(e), Some(PanelKind::RunResult))
        })
        .count();
    assert_eq!(
        count, 0,
        "サイドバーに PanelKind::RunResult のボタンは存在しないはず"
    );
}
