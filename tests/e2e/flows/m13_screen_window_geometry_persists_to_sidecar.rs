//! M13 screen_window_geometry_persists_to_sidecar — screen-space window
//! (`ScreenWindowRoot` + Bevy UI `Node`) の位置・サイズが Save でサイドカー JSON に
//! 永続化されることを保証する（kind:integration / ADR 0003 follow-up）。
//!
//! # 背景（このテストが守る回帰）
//! ADR 0003 で Strategy Editor / Startup は world-space sprite から screen-space `Node`
//! window へ移行した。`build_layout` は当初 `&Sprite`+`&Transform` の world-space window
//! しか走査しなかったため、screen window の left/top/width/height が **save されず**、
//! ドラッグした editor / Startup の位置が再オープンで失われていた（#35 A-2 follow-up）。
//! 本テストは「screen window が `windows` 配列に Node geometry 付きで現れる」ことを assert する。
//!
//! # 駆動経路（i7 を踏襲）
//! `StrategyBuffer.original_path` を temp `.py` に pre-seed → `LayoutSaveRequested` →
//! `handle_save_layout_system` が `.json` を書く。書かれた JSON の `windows` を検証する。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    InstrumentRegistry, PanelKind, ScenarioMetadata, ScenarioReadTarget, ScenarioWritebackPaths,
    StrategyBuffer, StrategyEditorId, StrategyFragment, WindowRoot,
};
use backcast::ui::layout_persistence::{
    handle_save_layout_system, LayoutSaveAsRequested, LayoutSaveRequested, PendingFileDialog,
};
use backcast::ui::screen_window::ScreenWindowRoot;
use backcast::ui::strategy_editor::StrategyAutoSaveState;

/// `BACKCAST_CACHE_DIR` を test 用に差し替え、Drop で元へ戻す RAII ガード。
struct CacheDirGuard(Option<OsString>);

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var("BACKCAST_CACHE_DIR", v),
                None => std::env::remove_var("BACKCAST_CACHE_DIR"),
            }
        }
    }
}

fn screen_node(left: f32, top: f32, w: f32, h: f32) -> Node {
    Node {
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

#[test]
#[serial]
fn m13_screen_window_geometry_persists_to_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let py_path = dir.path().join("strat.py");
    let json_path = dir.path().join("strat.json");
    let cache_dir = dir.path().join("cache");

    std::fs::write(&py_path, "# strategy\n").unwrap();

    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(StrategyBuffer {
        original_path: Some(py_path.clone()),
        cache_path: None,
        last_merged_source: None,
    });
    app.insert_resource(ScenarioWritebackPaths {
        cache_sidecar: None,
    });
    {
        let mut reg = InstrumentRegistry::default();
        reg.editable = false; // scenario instruments を上書きしない
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
        Projection::Orthographic(OrthographicProjection::default_2d()),
    ));

    // screen-space Strategy Editor window（Node geometry）。
    app.world_mut().spawn((
        WindowRoot,
        ScreenWindowRoot,
        PanelKind::StrategyEditor,
        StrategyEditorId {
            region_key: "region_001".to_string(),
        },
        StrategyFragment {
            source: "# editor\n".to_string(),
            dirty: false,
        },
        screen_node(120.0, 80.0, 600.0, 400.0),
        GlobalZIndex(12),
        Visibility::Inherited,
    ));

    // screen-space Startup window。
    app.world_mut().spawn((
        WindowRoot,
        ScreenWindowRoot,
        PanelKind::Startup,
        screen_node(60.0, 500.0, 300.0, 250.0),
        GlobalZIndex(10),
        Visibility::Inherited,
    ));

    app.add_systems(Update, handle_save_layout_system);

    app.world_mut().send_event(LayoutSaveRequested);
    app.update();

    assert!(json_path.exists(), "Save 後に sidecar JSON が存在するはず");
    let body = std::fs::read_to_string(&json_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).expect("JSON として読めるはず");
    let windows = v["windows"].as_array().expect("windows 配列があるはず");

    let editor = windows
        .iter()
        .find(|w| w["kind"] == "StrategyEditor")
        .unwrap_or_else(|| panic!("StrategyEditor screen window が windows に保存されるはず: {v}"));
    assert_eq!(
        editor["position"].as_array().unwrap(),
        &serde_json::json!([120.0, 80.0]).as_array().unwrap().clone(),
        "Strategy Editor の position は Node の left/top"
    );
    assert_eq!(
        editor["size"].as_array().unwrap(),
        &serde_json::json!([600.0, 400.0]).as_array().unwrap().clone(),
        "Strategy Editor の size は Node の width/height"
    );

    let startup = windows
        .iter()
        .find(|w| w["kind"] == "Startup")
        .unwrap_or_else(|| panic!("Startup screen window が windows に保存されるはず: {v}"));
    assert_eq!(
        startup["position"].as_array().unwrap(),
        &serde_json::json!([60.0, 500.0]).as_array().unwrap().clone(),
        "Startup の position は Node の left/top"
    );
}
