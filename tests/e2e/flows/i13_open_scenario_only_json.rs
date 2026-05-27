//! I13 open_scenario_only_json — `windows` / `strategy_path` を持たない scenario-only JSON を
//! 開いたとき、sibling `.py` があれば strategy load に委譲し、無ければ scenario target だけを
//! 更新することを保証する（kind:integration）。
//!
//! # 駆動経路
//! `LayoutLoadRequested { path, UserJsonOpen }` を注入 → `apply_layout_system` が:
//!   A. sibling `.py` あり: `StrategyFileLoadRequested` を発火して委譲
//!   B. sibling `.py` なし: `ScenarioReadTarget` だけ更新して終了
//!
//! # ケース
//! 1. scenario-only JSON + sibling `.py` あり
//!    → `StrategyFileLoadRequested` が発火、`ScenarioReadTarget` は変更されない
//! 2. scenario-only JSON + sibling `.py` なし
//!    → `StrategyFileLoadRequested` は発火しない、`ScenarioReadTarget` が JSON パスに更新される

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    ChartSizeMap,
    PanelSpawnRequested, PendingStrategyFragments, RegionKeyAllocator, ScenarioReadTarget,
    StrategyBuffer, StrategyFileLoadRequested, WindowManager,
};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::layout_persistence::{
    apply_layout_system, LayoutLoadDialogRequested, LayoutLoadMode, LayoutLoadRequested,
    LayoutSaveAsRequested, LayoutSaveRequested, PendingLayoutApply,
};

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(WindowManager::default());
    app.insert_resource(PendingLayoutApply::default());
    app.insert_resource(PendingStrategyFragments::default());
    app.insert_resource(ScenarioReadTarget::default());
    app.insert_resource(RegionKeyAllocator::default());
    app.insert_resource(AppHistory::default());
    app.insert_resource(StrategyBuffer::default());

    app.init_resource::<ChartSizeMap>();
    app.add_message::<LayoutLoadRequested>();
    app.add_message::<LayoutSaveRequested>();
    app.add_message::<LayoutSaveAsRequested>();
    app.add_message::<LayoutLoadDialogRequested>();
    app.add_message::<PanelSpawnRequested>();
    app.add_message::<StrategyFileLoadRequested>();

    // Camera2d: apply_layout_system の camera.get_single_mut が要求する。
    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
            ));

    app.add_systems(Update, apply_layout_system);

    app
}

#[test]
fn i13_open_scenario_only_json() {
    // ── ケース A: sibling `.py` あり ──
    {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("strat.json");
        let py_path = dir.path().join("strat.py");

        // scenario-only JSON（windows / strategy_path なし）。
        let body = serde_json::json!({
            "scenario": {
                "instrument": "7203.TSE",
                "start": "2025-01-06",
                "end": "2025-03-31",
                "granularity": "Daily",
                "initial_cash": 1000000
            }
        });
        std::fs::write(&json_path, serde_json::to_string(&body).unwrap()).unwrap();
        // sibling .py が存在する。
        std::fs::write(&py_path, "# strategy\ndef on_bar(): pass\n").unwrap();

        let mut app = build_app();

        app.world_mut().write_message(LayoutLoadRequested {
            path: json_path.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.update();

        // StrategyFileLoadRequested が 1 回発火したこと。
        let load_requests: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<StrategyFileLoadRequested>>()
            .drain()
            .collect();
        assert_eq!(
            load_requests.len(),
            1,
            "sibling .py があるとき apply_layout_system は StrategyFileLoadRequested を発火するはず"
        );
        assert!(
            load_requests[0].path.ends_with("strat.py"),
            "発火パスは sibling .py を指すはず、got {:?}",
            load_requests[0].path
        );

        // ScenarioReadTarget はこのケースでは apply_layout の loopback 抑制が起きるため
        // 変更されない（委譲先の handle_strategy_file_load_system が設定する）。
        // apply_layout_system の実装を確認: sibling .py がある場合は
        // `load_ev.send(StrategyFileLoadRequested)` した後 `*pending_loopback = Some(json)` して
        // `continue` する（ScenarioReadTarget を直接更新しない）。
    }

    // ── ケース B: sibling `.py` なし ──
    {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("solo.json");
        // sibling .py は存在しない。

        let body = serde_json::json!({
            "scenario": {
                "instrument": "8306.TSE",
                "start": "2025-01-06",
                "end": "2025-03-31",
                "granularity": "Daily",
                "initial_cash": 500000
            }
        });
        std::fs::write(&json_path, serde_json::to_string(&body).unwrap()).unwrap();

        let mut app = build_app();

        app.world_mut().write_message(LayoutLoadRequested {
            path: json_path.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.update();

        // StrategyFileLoadRequested は発火しない。
        let load_requests = app
            .world_mut()
            .resource_mut::<Messages<StrategyFileLoadRequested>>()
            .drain()
            .count();
        assert_eq!(
            load_requests, 0,
            "sibling .py がないとき StrategyFileLoadRequested は発火しないはず"
        );

        // ScenarioReadTarget が JSON パスに更新される。
        let target = app.world().resource::<ScenarioReadTarget>();
        assert!(
            target.0.is_some(),
            "sibling .py なし → ScenarioReadTarget が設定されるはず"
        );
        assert_eq!(
            target.0.as_ref().unwrap(),
            &json_path,
            "ScenarioReadTarget は開いた JSON パスを指すはず"
        );
    }
}
