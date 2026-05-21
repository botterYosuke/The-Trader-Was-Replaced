//! I5 file_open_spawns_editor_and_chart — replay モードで .json レイアウトを開くと
//! Strategy Editor と Chart のパネル entity が spawn することを保証する（kind:integration）。
//!
//! 実 seam を headless で駆動する:
//! - temp `.json`（StrategyEditor window を含む / strategy_path 無し）を書き、
//!   `LayoutLoadRequested{UserJsonOpen}` を注入 → 本番の `apply_layout_system` が
//!   ファイルを読んで `PanelSpawnRequested{StrategyEditor}` を送出 → 本番の
//!   `panel_spawn_dispatcher_system` が Strategy Editor entity（`StrategyEditorId`）を spawn。
//! - Chart は別系統: scenario ロードで埋まる `InstrumentRegistry` を起点に本番の
//!   `instrument_chart_sync_system` が銘柄ごとの `ChartInstrument` entity を spawn。
//!   ここでは registry を scenario 由来の値として注入する（JSON→registry の parse 自体は
//!   scenario_parser の単体テストが担保）。
//!
//! cosmic_edit はフォント resource（`CosmicFontSystem`）のみ手挿入し、描画はしない
//! （headless なので glyph は出ないが entity spawn には影響しない）。
//! 詳細は `tests/e2e/FLOWS.md` の I5 を参照。

use bevy::prelude::*;
use bevy_cosmic_edit::prelude::CosmicFontSystem;
use cosmic_text::FontSystem;

use backcast::trading::InstrumentTradingDataMap;
use backcast::ui::components::{
    ChartInstrument, InstrumentRegistry, PanelSpawnRequested, PendingStrategyFragments,
    RegionKeyAllocator, ScenarioReadTarget, StrategyBuffer, StrategyEditorId,
    StrategyFileLoadRequested, WindowManager, WindowRoot,
};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::{
    apply_layout_system, LayoutLoadMode, LayoutLoadRequested, PendingLayoutApply,
};
use backcast::ui::window::instrument_chart_sync_system;

#[test]
fn i5_file_open_spawns_editor_and_chart() {
    // 開く対象の .json: StrategyEditor window を持ち strategy_path は無し。
    // この形だと apply_layout_system は同フレームで PanelSpawnRequested を直接送る
    // （strategy_path 付きは .py ロード待ちに defer される別経路）。
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("layout.json");
    let body = serde_json::json!({
        "windows": [{
            "kind": "StrategyEditor",
            "position": [0.0, 0.0],
            "size": [400.0, 300.0],
            "z": 1.0,
            "visible": true,
            "region_key": "region_001"
        }],
        "strategy_path": null,
        "viewport": null
    });
    std::fs::write(&json_path, serde_json::to_string(&body).unwrap()).unwrap();

    let mut app = App::new();

    // apply_layout_system が触る resource。
    app.insert_resource(WindowManager::default())
        .insert_resource(PendingLayoutApply::default())
        .insert_resource(PendingStrategyFragments::default())
        .init_resource::<ScenarioReadTarget>()
        // panel_spawn_dispatcher_system が触る resource。
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(CosmicFontSystem(FontSystem::new()))
        // instrument_chart_sync_system が触る resource。
        // InstrumentRegistry は scenario ロードで埋まる値を注入（replay scenario の銘柄）。
        .insert_resource(InstrumentRegistry {
            ids: vec!["7203.TSE".to_string()],
            editable: true,
        })
        .insert_resource(InstrumentTradingDataMap::default());

    app.add_event::<LayoutLoadRequested>()
        .add_event::<PanelSpawnRequested>()
        .add_event::<StrategyFileLoadRequested>();

    // apply_layout_system はカメラを get_single_mut するため Camera2d を 1 体置く。
    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
        OrthographicProjection::default_2d(),
    ));

    // chain で apply_layout → dispatcher → chart sync の順を固定（同フレームで spawn まで到達）。
    app.add_systems(
        Update,
        (
            apply_layout_system,
            panel_spawn_dispatcher_system,
            instrument_chart_sync_system,
        )
            .chain(),
    );

    app.world_mut().send_event(LayoutLoadRequested {
        path: json_path.clone(),
        mode: LayoutLoadMode::UserJsonOpen,
    });

    // 1 フレームで spawn まで到達するはずだが、event の取りこぼし防止に複数回回す。
    app.update();
    app.update();

    let mut editor_q =
        app.world_mut()
            .query_filtered::<(), (With<StrategyEditorId>, With<WindowRoot>)>();
    let editor_count = editor_q.iter(app.world()).count();
    assert!(
        editor_count >= 1,
        "JSON を開くと Strategy Editor が spawn するはず (got {editor_count})"
    );

    let mut chart_q = app
        .world_mut()
        .query_filtered::<&ChartInstrument, With<WindowRoot>>();
    let charts: Vec<String> = chart_q
        .iter(app.world())
        .map(|c| c.instrument_id.clone())
        .collect();
    assert!(
        charts.iter().any(|id| id == "7203.TSE"),
        "scenario 銘柄に対応する Chart が spawn するはず (got {charts:?})"
    );
}
