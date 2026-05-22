//! M9 startup_window_position_persists_visible_not_authoritative —
//! `WindowLayout` 復元時、Startup ウィンドウは位置 / z は復元するが、保存された
//! `visible` で `Visibility` を上書きしないことを保証する（kind:ui）。
//!
//! Startup は `PanelKind::Startup` として layout 永続化に乗るが、可視性は
//! `ExecutionMode` が所有する（[M8] と対）。`visible:false` を含む layout を
//! `apply_layout_system` で復元しても Hidden に強制されてはならない（ADR-0001）。

use bevy::prelude::*;

use backcast::ui::components::{
    PanelKind, PanelSpawnRequested, PendingStrategyFragments, ScenarioReadTarget,
    StrategyFileLoadRequested, WindowManager, WindowRoot,
};
use backcast::ui::layout_persistence::{
    apply_layout_system, LayoutLoadMode, LayoutLoadRequested, PendingLayoutApply, SCHEMA_VERSION,
};

#[test]
fn m9_startup_window_position_persists_visible_not_authoritative() {
    let mut app = App::new();
    app.add_event::<LayoutLoadRequested>();
    app.add_event::<PanelSpawnRequested>();
    app.add_event::<StrategyFileLoadRequested>();
    app.insert_resource(WindowManager::default());
    app.insert_resource(PendingLayoutApply::default());
    app.insert_resource(PendingStrategyFragments::default());
    app.init_resource::<ScenarioReadTarget>();

    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
        OrthographicProjection::default_2d(),
    ));

    // Startup root は ExecutionMode が可視性を所有する。restore 前は Inherited。
    let startup = app
        .world_mut()
        .spawn((
            WindowRoot,
            PanelKind::Startup,
            Transform::from_xyz(0.0, 0.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(100.0, 100.0)),
                ..default()
            },
            Visibility::Inherited,
        ))
        .id();

    let tmp = std::env::temp_dir().join(format!(
        "ttwr_m9_startup_vis_{}.json",
        std::process::id()
    ));
    // visible:false を含む layout。pos/z は復元、visible は無視されるべき。
    let layout_json = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "viewport": null,
        "strategy_path": null,
        "windows": [{
            "kind": "Startup",
            "position": [42.0, 24.0],
            "size": [100.0, 100.0],
            "z": 7.0,
            "visible": false,
            "region_key": null
        }]
    });
    std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

    app.world_mut().send_event(LayoutLoadRequested {
        path: tmp.clone(),
        mode: LayoutLoadMode::UserJsonOpen,
    });
    app.add_systems(Update, apply_layout_system);
    app.update();

    let _ = std::fs::remove_file(&tmp);

    let vis = app.world().get::<Visibility>(startup).unwrap();
    assert!(
        !matches!(vis, Visibility::Hidden),
        "restore は Startup の Visibility を Hidden に強制してはいけない \
         (可視性は ExecutionMode が所有する)"
    );
    let tf = app.world().get::<Transform>(startup).unwrap();
    assert_eq!(tf.translation.x, 42.0, "Startup の位置 x は復元される");
    assert_eq!(tf.translation.y, 24.0, "Startup の位置 y は復元される");
    assert_eq!(tf.translation.z, 7.0, "Startup の z は復元される");
}
