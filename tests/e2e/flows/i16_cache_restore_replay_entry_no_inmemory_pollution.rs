//! I16 cache_restore_replay_entry_no_inmemory_pollution — 起動時 cache 復元の直後に
//! Replay へ入っても、セッション内 in-memory state（`StrategyBuffer.original_path` /
//! `PendingStrategyFragments.by_region_key`）が scenario `.json` sidecar の内容で
//! 汚染されないことを保証する（kind:integration）。
//!
//! # 背景（このテストが守るバグ・i15 と非重複の観点）
//! i15 は `app_state.py`（永続ファイル）が JSON で上書きされないことを守る。
//! 本フローはその一段手前、**セッション内メモリ汚染**を守る:
//! `restore_fixed_registry_on_replay_entry_system`（`src/ui/restore.rs`）は Replay 突入時に
//! `editable=false` の registry を再 resolve するため
//! `StrategyFileLoadRequested { path: ScenarioReadTarget.0(=.json), mode: LayoutRestore }`
//! を再送する。受け手 `handle_strategy_file_load_system`（`src/ui/menu_bar.rs`）は mode 分岐の
//! **手前で無条件に**:
//!   - `buffer.original_path = Some(event.path)` → `.json` を Python ソースパスとして記録
//!   - `pending.by_region_key` を clear し `split_py_into_fragments(.json 全文)` の結果を注入
//!     （JSON は `# region` マーカーを持たないため fallback で JSON 全文が `region_001` fragment 化）
//! を実行する。結果、Strategy Editor の in-memory buffer / pending fragments が JSON で汚染される。
//!
//! # 駆動経路（i15 と同一の seed / system chain）
//! 1. `BACKCAST_CACHE_DIR` を temp に差し替え（`CacheDirGuard`）。
//! 2. temp に `app_state.json`（sidecar layout, windows あり） + `app_state.py`（Python ソース）。
//! 3. `ExecutionModeRes = Replay`（既定）、`InstrumentRegistry.editable = false`。
//! 4. `restore_last_strategy_system` → `apply_cache_restore_system`
//!    → `restore_fixed_registry_on_replay_entry_system` → `handle_strategy_file_load_system`
//!    を 1 フレーム chain で回す。
//!
//! # 観測（不変条件）
//! - `StrategyBuffer.original_path` が `.json` を指していない。
//! - `PendingStrategyFragments.by_region_key` のどの値も JSON sidecar 本文（"schema_version" 等）を含まない。

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
fn i16_cache_restore_replay_entry_no_inmemory_pollution() {
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
    app.init_resource::<backcast::ui::components::ChartSizeMap>();
    app.insert_resource(ExecutionModeRes {
        mode: ExecutionMode::Replay,
    });
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

    // 観測1: StrategyBuffer.original_path が .json を指していないこと。
    let buffer = app.world().resource::<StrategyBuffer>();
    if let Some(p) = &buffer.original_path {
        assert_ne!(
            p.extension().and_then(|e| e.to_str()),
            Some("json"),
            "StrategyBuffer.original_path must not be polluted with the scenario .json sidecar \
             after replay-entry restore, got: {p:?}"
        );
    }

    // 観測2: pending fragments のどの値も JSON sidecar 本文を含まないこと。
    let pending = app.world().resource::<PendingStrategyFragments>();
    for (key, body) in &pending.by_region_key {
        assert!(
            !body.contains("schema_version"),
            "PendingStrategyFragments.by_region_key[{key:?}] must not contain the scenario JSON \
             sidecar body after replay-entry restore, got: {body}"
        );
    }
}
