//! I19 file_open_sidecar_missing_strategy_path_loads_sibling_py — sidecar JSON の
//! `strategy_path` が存在しないとき（例: 別 OS の絶対パス）、JSON と同ディレクトリの
//! sibling `<stem>.py` が自動ロードされることを保証する（kind:integration）。
//!
//! wiki（getting-started.md / replay.md）が「サイドカー JSON を開くと同名 .py が
//! 自動ロードされる」と約束しているが、strategy_path が存在しないとき else ブランチで
//! warn & skip するだけで sibling フォールバックが無い。
//!
//! # 観測
//! - `StrategyBuffer.original_path` が sibling `.py` を指す（strategy がロードされた）
//! - `PendingStrategyFragments.by_region_key` が空のまま（stale fragments は不在）

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes, InstrumentTradingDataMap};
use backcast::ui::components::{
    InstrumentRegistry, PanelSpawnRequested, PendingStrategyFragments, RegionKeyAllocator,
    ScenarioClearedFromFile, ScenarioFileWatchState, ScenarioInstrumentsWritebackState,
    ScenarioLoadedFromFile, ScenarioMetadata, ScenarioReadTarget, StrategyBuffer,
    StrategyFileLoadRequested, WindowManager, sync_registry_from_scenario_loaded_system,
};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::{
    LayoutLoadMode, LayoutLoadRequested, LayoutSaveAsRequested, LayoutSaveRequested,
    PendingLayoutApply, apply_layout_system, apply_pending_layout_system,
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
fn i19_file_open_sidecar_missing_strategy_path_loads_sibling_py() {
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("strat.json");
    let sibling_py = dir.path().join("strat.py");

    // sibling .py を作る（最低限のダミー内容）。
    std::fs::write(&sibling_py, b"# dummy strategy\n").unwrap();

    // strategy_path に存在しない絶対パス（別 OS の macOS パス）を設定した sidecar JSON。
    // windows が含まれているため scenario-only 経路（L1116）には入らず、
    // strategy_path 存在確認の else ブランチ（L1229）に落ちる。
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
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(ExecutionModeRes {
        mode: ExecutionMode::Replay,
    })
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
    .init_resource::<backcast::ui::components::ChartSizeMap>()
    .init_resource::<bevy::input_focus::InputFocus>()
    .init_resource::<backcast::ui::strategy_editor_find::FindReplaceState>();

    app.add_message::<LayoutSaveRequested>()
        .add_message::<LayoutSaveAsRequested>()
        .add_message::<LayoutLoadRequested>()
        .add_message::<PanelSpawnRequested>()
        .add_message::<StrategyFileLoadRequested>()
        .add_message::<ScenarioLoadedFromFile>()
        .add_message::<ScenarioClearedFromFile>();

    app.world_mut().spawn((Camera2d, Transform::default()));

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
        )
            .chain(),
    );

    // strategy_path が存在しない sidecar JSON を UserJsonOpen で開く。
    app.world_mut().write_message(LayoutLoadRequested {
        path: json_path.clone(),
        mode: LayoutLoadMode::UserJsonOpen,
    });
    app.update();

    // sibling .py がロードされたこと: StrategyBuffer.original_path が strat.py を指す。
    // apply_layout_system でフォールバック StrategyFileLoadRequested が write され、
    // 同 chain 内の handle_strategy_file_load_system が original_path を設定する。
    // 現実装ではフォールバックが無いため None のまま → この assert で RED になる。
    let buf = app.world().resource::<StrategyBuffer>();
    assert!(
        buf.original_path
            .as_ref()
            .map(|p| p.file_name() == sibling_py.file_name())
            .unwrap_or(false),
        "strategy_path が存在しないとき sibling .py がロードされ \
         StrategyBuffer.original_path に設定されるはず \
         (got={:?}, expected sibling={:?})",
        buf.original_path,
        sibling_py,
    );
}
