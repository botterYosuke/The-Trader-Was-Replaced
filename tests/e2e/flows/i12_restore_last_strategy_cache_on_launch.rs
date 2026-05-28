//! I12 restore_last_strategy_cache_on_launch — 起動時に `$BACKCAST_CACHE_DIR/app_state.json` /
//! `app_state.py` があれば、前回の strategy fragments / layout / scenario target を
//! 復元することを保証する（kind:integration）。
//!
//! # 駆動経路
//! 1. `BACKCAST_CACHE_DIR` を temp に差し替え（`CacheDirGuard`）。
//! 2. temp に `app_state.json`（sidecar layout） + `app_state.py`（strategy source）を書く。
//! 3. `restore_last_strategy_system` を Update で 1 フレーム回す
//!    → `CacheRestoreRequested` が発火。
//! 4. `apply_cache_restore_system` を Update で 1 フレーム回す
//!    → `StrategyBuffer` / `ScenarioReadTarget` / `PendingStrategyFragments` が更新される。
//!
//! # 観測
//! - `StrategyBuffer.original_path` が cache JSON の `strategy_path` フィールドを指す
//! - `ScenarioReadTarget.0` が `app_state.json` を指す（cache truth source）
//! - `PendingStrategyFragments` が fragments を持つ
//! - `CacheRestoreRequested` が 1 回発火する

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    ChartSizeMap,
    PanelSpawnRequested, PendingStrategyFragments, RegionKeyAllocator, ScenarioReadTarget,
    StrategyBuffer,
};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::layout_persistence::{
    apply_cache_restore_system, CacheRestoreRequested, PendingLayoutApply,
};
use backcast::ui::menu_bar::restore_last_strategy_system;

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
fn i12_restore_last_strategy_cache_on_launch() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().to_path_buf();

    let cache_json = cache_dir.join("app_state.json");
    let cache_py = cache_dir.join("app_state.py");

    // cache py: 1 region の strategy ソース。
    std::fs::write(&cache_py, "# strategy region_001\ndef on_bar(): pass\n").unwrap();

    // cache json: strategy_path を指す sidecar layout。
    // strategy_path は元の .py パス（restore 後 StrategyBuffer.original_path に入る）。
    let original_py = dir.path().join("original_strat.py");
    let sidecar_body = serde_json::json!({
        "schema_version": 1,
        "strategy_path": original_py.to_str().unwrap(),
        "windows": [{
            "kind": "StrategyEditor",
            "position": [0.0, 0.0],
            "size": [400.0, 300.0],
            "z": 1.0,
            "visible": true,
            "region_key": "region_001"
        }],
        "viewport": { "pan_x": 0.0, "pan_y": 0.0, "zoom": 1.0 },
        "scenario": {
            "instrument": "7203.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000
        }
    });
    std::fs::write(&cache_json, serde_json::to_string(&sidecar_body).unwrap()).unwrap();

    // BACKCAST_CACHE_DIR を temp に隔離。
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    // restore_last_strategy_system / apply_cache_restore_system が要求する resource。
    app.insert_resource(StrategyBuffer::default());
    app.insert_resource(PendingStrategyFragments::default());
    app.insert_resource(ScenarioReadTarget::default());
    app.insert_resource(RegionKeyAllocator::default());
    app.insert_resource(PendingLayoutApply::default());
    app.insert_resource(AppHistory::default());
    app.init_resource::<backcast::ui::components::ChartSizeMap>();

    app.init_resource::<ChartSizeMap>();
    app.add_message::<CacheRestoreRequested>();
    app.add_message::<PanelSpawnRequested>();

    // Camera2d がないと apply_cache_restore_system の camera.get_single_mut が
    // 失敗するだけ（panic しない）。
    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
            ));

    // restore → apply の順でチェーン。
    app.add_systems(
        Update,
        (restore_last_strategy_system, apply_cache_restore_system).chain(),
    );

    // ── Phase 1: restore_last_strategy_system が CacheRestoreRequested を発火 ──
    app.update();

    // CacheRestoreRequested はチェーンで走るため apply_cache_restore_system が消費する。
    // 発火した証拠: StrategyBuffer / ScenarioReadTarget が更新されているか確認する。
    let _ = app
        .world_mut()
        .resource_mut::<Messages<CacheRestoreRequested>>()
        .drain()
        .count();

    // ── 観測: StrategyBuffer.original_path が元 .py を指す ──
    let buffer = app.world().resource::<StrategyBuffer>();
    assert!(
        buffer.original_path.is_some(),
        "cache restore 後は StrategyBuffer.original_path が設定されるはず"
    );
    let restored_path = buffer.original_path.as_ref().unwrap();
    assert!(
        restored_path.to_str().unwrap().ends_with("original_strat.py"),
        "original_path は sidecar の strategy_path (original_strat.py) を指すはず、got {:?}",
        restored_path
    );

    // ── 観測: ScenarioReadTarget が cache_json を指す ──
    let target = app.world().resource::<ScenarioReadTarget>();
    assert!(
        target.0.is_some(),
        "cache restore 後は ScenarioReadTarget が設定されるはず"
    );
    assert!(
        target.0.as_ref().unwrap().ends_with("app_state.json"),
        "ScenarioReadTarget は app_state.json を指すはず、got {:?}",
        target.0
    );

    // ── 観測: PendingStrategyFragments に region_001 が含まれる ──
    let fragments = app.world().resource::<PendingStrategyFragments>();
    assert!(
        !fragments.by_region_key.is_empty(),
        "cache restore 後は PendingStrategyFragments にフラグメントが入るはず"
    );
    assert!(
        fragments.by_region_key.contains_key("region_001"),
        "region_001 キーがフラグメントに含まれるはず（got {:?}）",
        fragments.by_region_key.keys().collect::<Vec<_>>()
    );
}
