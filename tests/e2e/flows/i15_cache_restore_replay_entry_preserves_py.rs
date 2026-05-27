//! I15 cache_restore_replay_entry_preserves_py — 起動時 cache 復元の直後に Replay へ入っても、
//! `app_state.py` が scenario `.json` の中身で上書きされないことを保証する（kind:integration）。
//!
//! # 背景（このテストが守るバグ）
//! `restore_fixed_registry_on_replay_entry_system`（`src/ui/restore.rs`）は Replay 突入時に
//! `editable=false`（instruments_ref ロック）の registry を再 resolve するため
//! `StrategyFileLoadRequested { path: ScenarioReadTarget.0, .. }` を再送する。だが
//! `ScenarioReadTarget.0` は cache 復元後 `app_state.json`（scenario sidecar）を指す。
//! 受け手 `handle_strategy_file_load_system` はそのパスを **Python ソース** として扱い、
//! `split_py_into_fragments` でエディタへ流し込み、さらに `sync_to_cache` が
//! `std::fs::copy(app_state.json, app_state.py)` で **`app_state.py` を JSON で上書き**する。
//! 結果、次回起動で `apply_cache_restore_system` が JSON を読み、Strategy Editor に
//! `.json` の中身が表示される（自己永続化するキャッシュ破壊）。
//!
//! # 駆動経路
//! 1. `BACKCAST_CACHE_DIR` を temp に差し替え（`CacheDirGuard`）。
//! 2. temp に `app_state.json`（sidecar layout） + `app_state.py`（Python ソース）を書く。
//! 3. `ExecutionModeRes = Replay`（既定）、`InstrumentRegistry.editable = false`。
//! 4. `restore_last_strategy_system` → `apply_cache_restore_system`
//!    → `restore_fixed_registry_on_replay_entry_system` → `handle_strategy_file_load_system`
//!    を 1 フレーム chain で回す。
//!
//! # 観測（不変条件）
//! - `app_state.py` の中身は復元後も Python ソースのまま（JSON で上書きされない）。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes};
use backcast::ui::components::{
    ChartSizeMap,
    InstrumentRegistry, PanelSpawnRequested, PendingStrategyFragments, RegionKeyAllocator,
    ScenarioFileWatchState, ScenarioReadTarget, StrategyBuffer, StrategyFileLoadRequested,
};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::layout_persistence::{
    apply_cache_restore_system, CacheRestoreRequested, LayoutLoadRequested, PendingLayoutApply,
};
use backcast::ui::menu_bar::{handle_strategy_file_load_system, restore_last_strategy_system};
use backcast::ui::restore::restore_fixed_registry_on_replay_entry_system;

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
fn i15_cache_restore_replay_entry_preserves_py() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().to_path_buf();

    let cache_json = cache_dir.join("app_state.json");
    let cache_py = cache_dir.join("app_state.py");

    let py_source = "# strategy region_001\ndef on_bar():\n    pass\n";
    std::fs::write(&cache_py, py_source).unwrap();

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
            "instruments_ref": "universe_a",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000
        }
    });
    std::fs::write(&cache_json, serde_json::to_string(&sidecar_body).unwrap()).unwrap();

    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(StrategyBuffer::default());
    app.insert_resource(PendingStrategyFragments::default());
    app.insert_resource(ScenarioReadTarget::default());
    app.insert_resource(RegionKeyAllocator::default());
    app.init_resource::<ScenarioFileWatchState>();
    app.insert_resource(PendingLayoutApply::default());
    app.insert_resource(AppHistory::default());
    // 既定モードは Replay。最初の frame で None→Replay 遷移として entered_replay=true になる。
    app.insert_resource(ExecutionModeRes {
        mode: ExecutionMode::Replay,
    });
    // instruments_ref ロック = editable:false。これが restore.rs の再送をアームする。
    app.insert_resource(InstrumentRegistry {
        ids: vec!["LOCKED.T".to_string()],
        editable: false,
    });

    app.init_resource::<ChartSizeMap>();
    app.add_message::<CacheRestoreRequested>();
    app.add_message::<PanelSpawnRequested>();
    app.add_message::<StrategyFileLoadRequested>();
    app.add_message::<LayoutLoadRequested>();

    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
            ));

    app.add_systems(
        Update,
        (
            restore_last_strategy_system,
            apply_cache_restore_system,
            restore_fixed_registry_on_replay_entry_system,
            handle_strategy_file_load_system,
        )
            .chain(),
    );

    app.update();

    let py_after = std::fs::read_to_string(&cache_py).unwrap();
    assert!(
        !py_after.trim_start().starts_with('{'),
        "app_state.py must never be overwritten with the scenario JSON sidecar after \
         replay-entry restore, but it now starts with '{{': {py_after}"
    );
    assert!(
        py_after.contains("def on_bar"),
        "app_state.py must remain the original Python source after replay-entry restore, \
         got: {py_after}"
    );
}
