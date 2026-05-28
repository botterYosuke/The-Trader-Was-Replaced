//! I5 file_open_spawns_editor_and_chart — replay モードで Ctrl+O から戦略 `.json` を開くと
//! Strategy Editor と Chart が「実際に画面に出る」ことを保証する（kind:integration）。
//!
//! 実ユーザー操作（Ctrl+O → ダイアログでファイル選択 → 戦略 sidecar を開く）に忠実な経路を
//! headless で駆動する。OS ダイアログ（rfd）だけはバイパスし、その前後は本番 system を通す:
//! - **Ctrl+O ジェスチャ**: `ButtonInput<KeyCode>` に Ctrl+O を入れ、本番 `layout_shortcut_system`
//!   が `LayoutLoadDialogRequested` を発火することを assert（メニュー/ショートカットの seam）。
//! - **ダイアログのバイパス**: rfd を呼ぶ `handle_load_dialog_system` は走らせず、「ユーザーが
//!   ファイルを選んだ」結果＝`LayoutLoadRequested{path, UserJsonOpen}` を直接注入する。
//! - **Strategy Editor（実 .py ロード経路）**: `apply_layout_system` が `strategy_path` を見て
//!   `.py` ロードを要求 → 本番 `handle_strategy_file_load_system` が **実際に `.py` を読み**
//!   fragment 分割して `PendingStrategyFragments` を埋め → `apply_pending_layout_system` →
//!   `panel_spawn_dispatcher_system` が editor を spawn。
//!   ※ ローダは cache（実アプリ共有の `app_state.*`）へ書き込むため、`BACKCAST_CACHE_DIR` を
//!     temp に逃がして実 cache を汚さない。
//! - **Chart（scenario 経路）**: ローダが `ScenarioReadTarget` に sibling `.json` をセット →
//!   `parse_scenario_system` が `scenario` を読み `ScenarioLoadedFromFile` →
//!   `sync_registry_from_scenario_loaded_system` が `InstrumentRegistry` を埋め →
//!   `instrument_chart_sync_system` が Chart を spawn。
//!
//! 検証は目視ではなく構造化 UI ダンプ（位置 / 大きさ / 表示 / キャプション）で行う。
//! cosmic_edit はフォント resource のみ headless 挿入（描画はしない）。実ピクセル描画の smoke は
//! `L4`（headless 不可）。詳細は `tests/e2e/FLOWS.md` の I5 を参照。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy_cosmic_edit::prelude::CosmicFontSystem;
use cosmic_text::FontSystem;

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
    apply_layout_system, apply_pending_layout_system, layout_shortcut_system,
    LayoutLoadDialogRequested, LayoutLoadMode, LayoutLoadRequested, LayoutSaveAsRequested,
    LayoutSaveRequested, PendingLayoutApply,
};
use backcast::ui::menu_bar::handle_strategy_file_load_system;
use backcast::ui::scenario_parser::parse_scenario_system;
use backcast::ui::window::instrument_chart_sync_system;

use crate::ui_dump::{dump_elements, dump_panels, panels_of, ElementKind};

/// `BACKCAST_CACHE_DIR` を test 用に差し替え、Drop で元へ戻す RAII ガード。
/// strategy ローダの cache 書き込みを temp に隔離するため。
struct CacheDirGuard(Option<OsString>);

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        // SAFETY: テスト終了時に env を元へ戻すだけ。値読み取りと競合しない単一地点で実行する。
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
fn i5_file_open_spawns_editor_and_chart() {
    let dir = tempfile::tempdir().unwrap();
    let py_path = dir.path().join("strat.py");
    let json_path = dir.path().join("strat.json");
    std::fs::write(&py_path, "# strategy\n").unwrap();

    // 戦略 sidecar: strategy_path（→ .py）/ scenario（銘柄）/ windows（StrategyEditor）。
    let body = serde_json::json!({
        "strategy_path": py_path.to_str().unwrap(),
        "windows": [{
            "kind": "StrategyEditor",
            "position": [0.0, 0.0],
            "size": [400.0, 300.0],
            "z": 1.0,
            "visible": true,
            "region_key": "region_001"
        }],
        "scenario": {
            "instrument": "7203.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000
        }
    });
    std::fs::write(&json_path, serde_json::to_string(&body).unwrap()).unwrap();

    // strategy ローダの cache 書き込みを temp に逃がす（実 cache を汚さない）。
    // SAFETY: app 構築前の単一地点で設定し、ガードの Drop で復元する。
    let cache_dir = dir.path().join("cache");
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    // 子要素の絶対座標を出すため Transform 伝播を有効化（render は不要）。
    app.add_plugins(TransformPlugin);

    app
        // 「replay モードで」の前提を固定。
        .insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        })
        // Ctrl+O ジェスチャ（layout_shortcut_system）が触る resource。
        .insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<()>::default())
        // apply_layout / loader / apply_pending が触る resource。
        .insert_resource(WindowManager::default())
        .insert_resource(PendingLayoutApply::default())
        .insert_resource(PendingStrategyFragments::default())
        // ScenarioReadTarget は loader が sibling .json をセットする（初期は None）。
        .insert_resource(ScenarioReadTarget::default())
        // dispatcher / loader が触る resource。
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(CosmicFontSystem(FontSystem::new()))
        // scenario → registry → chart が触る resource。
        .insert_resource(ScenarioMetadata::default())
        .insert_resource(ScenarioFileWatchState::default())
        .insert_resource(ScenarioInstrumentsWritebackState::default())
        .insert_resource(InstrumentRegistry::default())
        .insert_resource(InstrumentTradingDataMap::default())
        .init_resource::<backcast::ui::components::ChartSizeMap>();

    app.add_message::<LayoutLoadDialogRequested>()
        .add_message::<LayoutSaveRequested>()
        .add_message::<LayoutSaveAsRequested>()
        .add_message::<LayoutLoadRequested>()
        .add_message::<PanelSpawnRequested>()
        .add_message::<StrategyFileLoadRequested>()
        .add_message::<ScenarioLoadedFromFile>()
        .add_message::<ScenarioClearedFromFile>();

    // apply_layout_system はカメラを get_single_mut するため Camera2d を 1 体置く。
    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
            ));

    app.add_systems(
        Update,
        (
            layout_shortcut_system,
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

    // ── Phase A: Ctrl+O ジェスチャ → ダイアログ要求の発火 ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyO);
    }
    app.update();

    // Ctrl+O で本番 layout_shortcut_system がダイアログ要求を発火したこと。
    let dialog_requests = app
        .world_mut()
        .resource_mut::<Messages<LayoutLoadDialogRequested>>()
        .drain()
        .count();
    assert_eq!(
        dialog_requests, 1,
        "Ctrl+O で LayoutLoadDialogRequested が発火するはず"
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();

    // load 前: まだ何も開いていない。
    {
        let panels = dump_panels(app.world_mut());
        assert!(
            panels_of(&panels, "Strategy Editor").is_empty() && panels_of(&panels, "Chart").is_empty(),
            "load 前は Strategy Editor も Chart も出ない (panels={panels:#?})"
        );
    }

    // ── Phase B: rfd ダイアログをバイパスし「選択された path」を注入 ──
    // 1 フレームで apply_layout → 実 .py ローダ → deferred spawn → scenario → chart が連鎖する。
    app.world_mut().write_message(LayoutLoadRequested {
        path: json_path.clone(),
        mode: LayoutLoadMode::UserJsonOpen,
    });
    app.update();

    assert_eq!(
        app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::Replay,
        "テストは replay モード前提"
    );

    // 目視の代わりに構造化 UI ダンプで位置 / 大きさ / 表示 / キャプションを確認する。
    let panels = dump_panels(app.world_mut());

    let editors = panels_of(&panels, "Strategy Editor");
    assert_eq!(
        editors.len(),
        1,
        "実 .py ロード経由で Strategy Editor が 1 枚 spawn するはず (panels={panels:#?})"
    );
    let editor = editors[0];
    assert!(editor.visible, "Strategy Editor は表示状態のはず");
    assert!(
        editor.size.x > 0.0 && editor.size.y > 0.0,
        "Strategy Editor はサイズを持つはず (got {:?})",
        editor.size
    );
    assert_eq!(editor.region_key.as_deref(), Some("region_001"));
    assert!(
        editor.has_caption_ci("strategy editor"),
        "Strategy Editor のタイトルキャプションがあるはず (captions={:?})",
        editor.captions
    );

    let charts = panels_of(&panels, "Chart");
    let chart = charts
        .iter()
        .find(|c| c.instrument_id.as_deref() == Some("7203.TSE"))
        .unwrap_or_else(|| panic!("7203.TSE の Chart が spawn するはず (panels={panels:#?})"));
    assert!(chart.visible, "Chart は表示状態のはず");
    assert!(
        chart.size.x > 0.0 && chart.size.y > 0.0,
        "Chart はサイズを持つはず (got {:?})",
        chart.size
    );
    assert!(
        chart.has_caption("7203.TSE"),
        "Chart のキャプションに銘柄が出るはず (captions={:?})",
        chart.captions
    );

    // 全要素ダンプ: 個々の描画要素を絶対座標付きで取れること（目視代替の網羅確認）。
    let elements = dump_elements(app.world_mut());
    assert!(
        elements.iter().any(|e| e.kind == ElementKind::Text
            && e.caption.as_deref().is_some_and(|c| c.contains("7203.TSE"))),
        "要素ダンプにチャートの銘柄キャプション(Text)が含まれるはず"
    );
    assert!(
        elements
            .iter()
            .filter(|e| e.kind == ElementKind::Sprite && e.size.is_some())
            .count()
            >= 2,
        "Strategy Editor と Chart の本体 Sprite が要素ダンプに出るはず (elements={})",
        elements.len()
    );
}
