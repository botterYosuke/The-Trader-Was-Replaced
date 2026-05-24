//! M14 screen_editor_window_geometry_restores — `apply_layout_system` が既存の
//! screen-space Strategy Editor window (`ScreenWindowRoot` + `Node`) に対し、保存された
//! position / size / visibility を復元することを保証する（kind:ui / ADR 0003 follow-up）。
//!
//! Startup（[M9]）と異なり Strategy Editor は **size も visible も layout が権威**を持つ
//! （ExecutionMode が所有するのは Startup の可視性のみ）。よって `visible:false` の layout は
//! editor の `Visibility` を `Hidden` にし、size は Node の width/height へ復元する。
//!
//! 復元は world-space match（`&Sprite`）が空振りした後に screen-space match（`&Node`）で
//! 行われる。実装が screen match を欠くと editor が見つからず spawn fallback に落ちて
//! Node が更新されない（= RED）。

use bevy::prelude::*;

use backcast::ui::components::{
    PanelKind, PanelSpawnRequested, PendingStrategyFragments, ScenarioReadTarget,
    StrategyEditorId, StrategyFileLoadRequested, WindowManager, WindowRoot,
};
use backcast::ui::layout_persistence::{
    apply_layout_system, LayoutLoadMode, LayoutLoadRequested, PendingLayoutApply, SCHEMA_VERSION,
};
use backcast::ui::screen_window::{px_of, ScreenWindowRoot};

#[test]
fn m14_screen_editor_window_geometry_restores() {
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

    // screen-space Strategy Editor window（region_001）。restore 前は左上 (0,0)・可視。
    let editor = app
        .world_mut()
        .spawn((
            WindowRoot,
            ScreenWindowRoot,
            PanelKind::StrategyEditor,
            StrategyEditorId {
                region_key: "region_001".to_string(),
            },
            Node {
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(600.0),
                height: Val::Px(400.0),
                ..default()
            },
            GlobalZIndex(10),
            Visibility::Inherited,
        ))
        .id();

    let tmp = std::env::temp_dir().join(format!("ttwr_m14_editor_{}.json", std::process::id()));
    let layout_json = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "viewport": null,
        "strategy_path": null,
        "windows": [{
            "kind": "StrategyEditor",
            "position": [200.0, 150.0],
            "size": [700.0, 500.0],
            "z": 15.0,
            "visible": false,
            "region_key": "region_001"
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

    let node = app.world().get::<Node>(editor).unwrap();
    assert_eq!(px_of(node.left), 200.0, "editor の left は復元される");
    assert_eq!(px_of(node.top), 150.0, "editor の top は復元される");
    assert_eq!(
        px_of(node.width),
        700.0,
        "editor の size (width) は layout が権威 → 復元される"
    );
    assert_eq!(
        px_of(node.height),
        500.0,
        "editor の size (height) は layout が権威 → 復元される"
    );
    let vis = app.world().get::<Visibility>(editor).unwrap();
    assert!(
        matches!(vis, Visibility::Hidden),
        "editor の visible は layout が権威 → visible:false で Hidden になる"
    );
    let z = app.world().get::<GlobalZIndex>(editor).unwrap();
    assert_eq!(z.0, 15, "editor の z (GlobalZIndex) は復元される");

    // editor が match されず spawn fallback に落ちていないこと（既存 1 件のまま）。
    let editor_count = app
        .world_mut()
        .query_filtered::<(), (With<PanelKind>, With<ScreenWindowRoot>)>()
        .iter(app.world())
        .count();
    assert_eq!(editor_count, 1, "既存 editor が match されるべき（重複 spawn しない）");
}
