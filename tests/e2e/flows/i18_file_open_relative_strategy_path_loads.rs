//! I18 file_open_relative_strategy_path_loads — `strategy_path` が相対パス
//! （`"examples/test_strategy_minute.py"`）のとき、プロセスの CWD（`cargo test` では
//! workspace root）基準で解決され、strategy が正しくロードされることを保証する（kind:integration）。
//!
//! # なぜ既存テストで検知できなかったか
//! I5 / I12 は `tempdir().join(...)` で生成した OS ネイティブの**絶対パス**しか
//! `strategy_path` に設定しない。相対パスが CWD 基準で解決されることを確認するテストが無かった。
//! `test_strategy_minute.json` の `strategy_path` が macOS 絶対パスで保存されていたため
//! I17 のバグが発生し、修正として相対パス `"examples/test_strategy_minute.py"` に変えた。
//! 本テストはその動作を回帰ガードとして固定する。
//!
//! # 前提
//! `cargo test` の CWD は workspace root（`The-Trader-Was-Replaced/`）。
//! `examples/test_strategy_minute.py` は workspace root からの相対パスで存在する。
//!
//! # 観測
//! - `PendingStrategyFragments.by_region_key["region_001"]` が埋まる（strategy がロードされた）
//! - Strategy Editor が spawn される

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes, InstrumentTradingDataMap};
use backcast::ui::components::{
    sync_registry_from_scenario_loaded_system, InstrumentRegistry, PanelSpawnRequested,
    RegionKeyAllocator, ScenarioClearedFromFile, ScenarioFileWatchState,
    ScenarioInstrumentsWritebackState, ScenarioLoadedFromFile, ScenarioMetadata, ScenarioReadTarget,
    StrategyBuffer, StrategyFileLoadRequested, WindowManager, PendingStrategyFragments,
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

use crate::ui_dump::{dump_panels, panels_of};

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
fn i18_file_open_relative_strategy_path_loads() {
    // 前提: この相対パスが cargo test の CWD（workspace root）から解決できること。
    let rel_path = "examples/test_strategy_minute.py";
    assert!(
        std::path::Path::new(rel_path).exists(),
        "examples/test_strategy_minute.py が workspace root から存在することが前提"
    );

    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("strat.json");

    let body = serde_json::json!({
        "schema_version": 1,
        "strategy_path": rel_path,
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
        .insert_resource(InstrumentTradingDataMap::default())
        .init_resource::<backcast::ui::components::ChartSizeMap>();

    app.add_message::<LayoutSaveRequested>()
        .add_message::<LayoutSaveAsRequested>()
        .add_message::<LayoutLoadRequested>()
        .add_message::<PanelSpawnRequested>()
        .add_message::<StrategyFileLoadRequested>()
        .add_message::<ScenarioLoadedFromFile>()
        .add_message::<ScenarioClearedFromFile>();

    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
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

    // 相対パスを持つ JSON を UserJsonOpen で開く。
    app.world_mut().write_message(LayoutLoadRequested {
        path: json_path.clone(),
        mode: LayoutLoadMode::UserJsonOpen,
    });
    app.update();

    // 相対パスが解決されて strategy がロードされたこと。
    // by_region_key は panel_spawn_dispatcher_system が消費するため、
    // 消費後も残る StrategyBuffer.original_path で検証する。
    let buf = app.world().resource::<StrategyBuffer>();
    assert!(
        buf.original_path
            .as_ref()
            .map(|p| p.to_str().unwrap_or("").ends_with("test_strategy_minute.py"))
            .unwrap_or(false),
        "相対 strategy_path が解決され StrategyBuffer.original_path に設定されるはず \
         (got={:?})",
        buf.original_path
    );

    // Strategy Editor が spawn されたこと。
    let panels = dump_panels(app.world_mut());
    let editors = panels_of(&panels, "Strategy Editor");
    assert_eq!(
        editors.len(),
        1,
        "相対 strategy_path で Strategy Editor が 1 枚 spawn するはず \
         (panels={panels:#?})"
    );
    assert!(editors[0].visible, "Strategy Editor は表示状態のはず");
}
