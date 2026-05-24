//! I17 file_open_bad_strategy_path_clears_stale_cache — `strategy_path` が存在しないファイルを
//! 指すとき（例: macOS 絶対パスを Windows で開く）、前セッションの stale な
//! `PendingStrategyFragments` が editor に残らないことを保証する（kind:integration）。
//!
//! # なぜ既存テストで検知できなかったか
//! I5 / I12 は temp dir で生成した**実際に存在する**絶対パスしか `strategy_path` に設定しない。
//! macOS で保存した JSON を Windows で開いた場合のように `strategy_path` が存在しないケースを
//! テストしていなかった。その結果 `apply_layout_system` の else ブランチ（warn + skip）が
//! `pending_fragments.by_region_key.clear()` を呼ばず、前セッションの cache 内容が
//! `# dummystrategy` のままエディターに残り続けた。
//!
//! # 観測
//! - `PendingStrategyFragments.by_region_key` が空になる（stale 内容がクリアされる）
//! - `PendingStrategyFragments.loaded_for_path` が None になる
//! - `StrategyFileLoadRequested` は送信されない

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes, InstrumentTradingDataMap};
use backcast::ui::components::{
    sync_registry_from_scenario_loaded_system, InstrumentRegistry, PanelSpawnRequested,
    PendingStrategyFragments, RegionKeyAllocator, ScenarioClearedFromFile, ScenarioFileWatchState,
    ScenarioInstrumentsWritebackState, ScenarioLoadedFromFile, ScenarioMetadata, ScenarioReadTarget,
    StrategyBuffer, StrategyFileLoadRequested, WindowManager,
};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::{
    apply_layout_system, apply_pending_layout_system, LayoutLoadMode, LayoutLoadRequested,
    LayoutSaveAsRequested, LayoutSaveRequested, PendingLayoutApply,
};
use backcast::ui::menu_bar::handle_strategy_file_load_system;
use backcast::ui::scenario_parser::parse_scenario_system;
use backcast::ui::window::instrument_chart_sync_system;

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
fn i17_file_open_bad_strategy_path_clears_stale_cache() {
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("strat.json");

    // macOS 絶対パスをシミュレート（このマシンでは存在しない）。
    let bad_path = "/Users/nonexistent_user/_blacksheep/The-Trader-Was-Replaced/examples/strat.py";
    assert!(
        !std::path::Path::new(bad_path).exists(),
        "bad_path は存在しないことが前提 (got: {:?})",
        bad_path
    );

    let body = serde_json::json!({
        "schema_version": 1,
        "strategy_path": bad_path,
        "windows": [{
            "kind": "StrategyEditor",
            "position": [0.0, 0.0],
            "size": [400.0, 300.0],
            "z": 1.0,
            "visible": true,
            "region_key": "region_001"
        }],
        "scenario": {
            "instruments": ["1301.TSE"],
            "start": "2025-01-06",
            "end": "2025-05-21",
            "granularity": "Minute",
            "initial_cash": 1000000,
            "schema_version": 2
        }
    });
    std::fs::write(&json_path, serde_json::to_string(&body).unwrap()).unwrap();

    let cache_dir = dir.path().join("cache");
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe { std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir); }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(ExecutionModeRes { mode: ExecutionMode::Replay })
        .insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<()>::default())
        .insert_resource(WindowManager::default())
        .insert_resource(PendingLayoutApply::default())
        .insert_resource(PendingStrategyFragments::default())
        .insert_resource(ScenarioReadTarget::default())
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(ScenarioMetadata::default())
        .insert_resource(ScenarioFileWatchState::default())
        .insert_resource(ScenarioInstrumentsWritebackState::default())
        .insert_resource(InstrumentRegistry::default())
        .insert_resource(InstrumentTradingDataMap::default());

    app.add_event::<LayoutSaveRequested>()
        .add_event::<LayoutSaveAsRequested>()
        .add_event::<LayoutLoadRequested>()
        .add_event::<PanelSpawnRequested>()
        .add_event::<StrategyFileLoadRequested>()
        .add_event::<ScenarioLoadedFromFile>()
        .add_event::<ScenarioClearedFromFile>();

    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
        Projection::Orthographic(OrthographicProjection::default_2d()),
    ));

    app.add_systems(
        Update,
        (
            apply_layout_system,
            handle_strategy_file_load_system,
            apply_pending_layout_system,
            panel_spawn_dispatcher_system,
            parse_scenario_system,
            sync_registry_from_scenario_loaded_system,
            instrument_chart_sync_system,
        ).chain(),
    );

    // 前セッションのキャッシュ復元から残った stale fragments を事前注入。
    {
        let mut frags = app.world_mut().resource_mut::<PendingStrategyFragments>();
        frags.by_region_key.insert("region_001".to_string(), "# dummystrategy\n".to_string());
        frags.loaded_for_path = Some(std::path::PathBuf::from("/old/stale/app_state.py"));
    }

    // 存在しない strategy_path を持つ JSON を UserJsonOpen で開く。
    app.world_mut().send_event(LayoutLoadRequested {
        path: json_path.clone(),
        mode: LayoutLoadMode::UserJsonOpen,
    });
    app.update();

    let frags = app.world().resource::<PendingStrategyFragments>();

    assert!(
        frags.by_region_key.is_empty(),
        "存在しない strategy_path を UserJsonOpen で開いたとき、\
         stale な fragments はクリアされるはず (got={:?})",
        frags.by_region_key.keys().collect::<Vec<_>>()
    );
    assert!(
        frags.loaded_for_path.is_none(),
        "loaded_for_path も None になるはず (got={:?})",
        frags.loaded_for_path
    );
}
