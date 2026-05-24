//! M9 startup_window_position_persists_visible_not_authoritative —
//! `WindowLayout` 復元時、Startup ウィンドウは位置 (left/top) / z は復元するが、保存された
//! `visible` で `Visibility` を上書きしない／size も復元しないことを保証する（kind:ui）。
//!
//! ADR 0003: Startup は world-space sprite から **screen-space window**（`ScreenWindowRoot`
//! + Bevy UI `Node`、left/top/width/height）へ移行した。よって復元は `Transform`/`Sprite` では
//! なく `Node.left`/`Node.top` へ書く（`GlobalZIndex` で z 順）。size は窓側定数が正なので
//! 復元しない。可視性は `ExecutionMode` が所有する（[M8] と対）ので `visible:false` を含む
//! layout を `apply_layout_system` で復元しても Hidden に強制されてはならない（ADR-0001）。

use bevy::prelude::*;

use backcast::ui::components::{
    PanelKind, PanelSpawnRequested, PendingStrategyFragments, ScenarioReadTarget,
    StrategyFileLoadRequested, WindowManager, WindowRoot,
};
use backcast::ui::layout_persistence::{
    apply_layout_system, LayoutLoadMode, LayoutLoadRequested, PendingLayoutApply, SCHEMA_VERSION,
};
use backcast::ui::screen_window::{px_of, ScreenWindowRoot};

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
        Projection::Orthographic(OrthographicProjection::default_2d()),
    ));

    // Startup root は screen-space window（ScreenWindowRoot + Node）。
    // 可視性は ExecutionMode が所有する。restore 前は Inherited。
    let startup = app
        .world_mut()
        .spawn((
            WindowRoot,
            ScreenWindowRoot,
            PanelKind::Startup,
            Node {
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(300.0),
                height: Val::Px(250.0),
                ..default()
            },
            GlobalZIndex(10),
            Visibility::Inherited,
        ))
        .id();

    let tmp = std::env::temp_dir().join(format!(
        "ttwr_m9_startup_vis_{}.json",
        std::process::id()
    ));
    // visible:false を含む layout。pos/z は復元、visible は無視・size も無視されるべき。
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
    let node = app.world().get::<Node>(startup).unwrap();
    assert_eq!(px_of(node.left), 42.0, "Startup の left は復元される");
    assert_eq!(px_of(node.top), 24.0, "Startup の top は復元される");
    assert_eq!(
        px_of(node.width),
        300.0,
        "Startup の size (width) は復元しない（窓側定数が正）"
    );
    assert_eq!(
        px_of(node.height),
        250.0,
        "Startup の size (height) は復元しない（窓側定数が正）"
    );
    let z = app.world().get::<GlobalZIndex>(startup).unwrap();
    assert_eq!(z.0, 7, "Startup の z (GlobalZIndex) は復元される");
}
