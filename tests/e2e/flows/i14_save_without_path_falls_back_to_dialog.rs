//! I14 save_without_path_falls_back_to_dialog — original_path 未確定で Ctrl+S（Save）を
//! 実行すると、Save As 相当のダイアログ起動へフォールバックすることを headless で検証する（kind:state）。
//!
//! 案 A: rfd を踏まずに済むよう、None 分岐は `LayoutSaveAsRequested` を emit する設計に委ねる。
//! ここでは `LayoutSaveRequested`（original_path=None）→ `handle_save_layout_system` が
//! `LayoutSaveAsRequested` を 1 回発火することを assert する（書き込み結果は別途）。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    InstrumentRegistry, ScenarioMetadata, ScenarioReadTarget, ScenarioWritebackPaths,
    StrategyBuffer,
};
use backcast::ui::strategy_editor::StrategyAutoSaveState;
use backcast::ui::layout_persistence::{
    handle_save_layout_system, LayoutSaveAsRequested, LayoutSaveRequested, PendingFileDialog,
};

#[test]
fn i14_save_without_path_falls_back_to_dialog() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    // original_path = None（未保存）の状態。
    app.insert_resource(StrategyBuffer {
        original_path: None,
        cache_path: None,
        last_merged_source: None,
    });
    app.insert_resource(ScenarioWritebackPaths { cache_sidecar: None });
    {
        let mut reg = InstrumentRegistry::default();
        reg.editable = false;
        app.insert_resource(reg);
    }
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<StrategyAutoSaveState>();
    app.init_resource::<ScenarioReadTarget>();
    app.init_resource::<PendingFileDialog>();

    app.add_event::<LayoutSaveRequested>();
    app.add_event::<LayoutSaveAsRequested>();

    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
        OrthographicProjection::default_2d(),
    ));

    app.add_systems(Update, handle_save_layout_system);

    // original_path=None で Save を要求 → Save As フォールバックが発火するはず。
    app.world_mut().send_event(LayoutSaveRequested);
    app.update();

    let save_as_count = app
        .world_mut()
        .resource_mut::<Events<LayoutSaveAsRequested>>()
        .drain()
        .count();
    assert_eq!(
        save_as_count, 1,
        "original_path=None の Save は Save As ダイアログ起動へフォールバックするはず"
    );
}
