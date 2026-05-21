//! I7 save_layout_writes_sidecar — Save が現在の viewport / window 配置 / strategy_path /
//! scenario を既存の `.json` サイドカーへ書き戻すことを保証する（kind:integration）。
//!
//! # 駆動経路
//! `StrategyBuffer.original_path` を temp `.py` に pre-seed（dialog バイパス）。
//! `LayoutSaveRequested` を発火 → `handle_save_layout_system` が `.json` を書く。
//! 書かれた JSON を読み取り `strategy_path` / `windows` / `schema_version` フィールドを検証する。
//!
//! # 注意
//! `handle_save_layout_system` は `cache_state_paths()` を内部で呼ぶため、
//! `BACKCAST_CACHE_DIR` を temp に逃がして実 cache を汚さない（`CacheDirGuard`）。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    InstrumentRegistry, ScenarioMetadata, ScenarioReadTarget, ScenarioWritebackPaths,
    StrategyBuffer,
};
use backcast::ui::layout_persistence::{
    handle_save_layout_system, LayoutSaveRequested,
    // LayoutSaveAsRequested だけは使わないが他の event が必要なシステムは無い
};
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

#[test]
#[serial]
fn i7_save_layout_writes_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let py_path = dir.path().join("strat.py");
    let json_path = dir.path().join("strat.json");
    let cache_dir = dir.path().join("cache");

    std::fs::write(&py_path, "# strategy\n").unwrap();

    // 既存サイドカー JSON: scenario キーを含む（Save が引き継ぐはず）。
    let initial_json = serde_json::json!({
        "schema_version": 1,
        "scenario": {
            "instrument": "7203.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000
        }
    });
    std::fs::write(&json_path, serde_json::to_string(&initial_json).unwrap()).unwrap();

    // cache 書き込みを temp に隔離。
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    // handle_save_layout_system が要求するすべての resource を挿入する。
    app.insert_resource(StrategyBuffer {
        original_path: Some(py_path.clone()),
        cache_path: None,
        last_merged_source: None,
    });
    app.insert_resource(ScenarioWritebackPaths {
        cache_sidecar: None, // cache sidecar なし → fallback で original json を保存
    });
    {
        let mut reg = InstrumentRegistry::default();
        reg.editable = false; // editable=false → scenario instruments を上書きしない
        app.insert_resource(reg);
    }
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<StrategyAutoSaveState>();
    app.init_resource::<ScenarioReadTarget>();

    app.add_event::<LayoutSaveRequested>();

    // Camera2d がないと build_layout 内の camera.get_single() が空を返すだけ（panic しない）。
    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
        OrthographicProjection::default_2d(),
    ));

    app.add_systems(Update, handle_save_layout_system);

    // ── Save を発火 ──
    app.world_mut().send_event(LayoutSaveRequested);
    app.update();

    // ── JSON ファイルが生成・更新されたことを確認 ──
    assert!(
        json_path.exists(),
        "Save 後に sidecar JSON {:?} が存在するはず",
        json_path
    );

    let body = std::fs::read_to_string(&json_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).expect("JSON として読めるはず");

    // schema_version フィールドが存在する。
    assert!(
        v.get("schema_version").is_some(),
        "保存 JSON に schema_version が存在するはず: {v}"
    );

    // strategy_path フィールドが `.py` パスを指す。
    let sp = v["strategy_path"].as_str().expect("strategy_path フィールドがあるはず");
    assert!(
        sp.ends_with("strat.py"),
        "strategy_path は strat.py を指すはず、got {sp}"
    );

    // windows フィールドが配列として存在する（パネルなしなら空配列）。
    assert!(
        v.get("windows").is_some(),
        "保存 JSON に windows フィールドが存在するはず: {v}"
    );
}
