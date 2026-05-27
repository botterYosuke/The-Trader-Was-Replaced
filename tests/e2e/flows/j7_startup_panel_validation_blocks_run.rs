//! J7 startup_panel_validation_blocks_run — Startup パネルの Start / End / Granularity / Initial cash が
//! 空・不正・範囲不整合のとき Run command を送らず、エラーが `ScenarioStartupParams.errors` にセットされることを保証する（kind:ui）。
//!
//! `commit_startup_params_to_scenario_system` で validation error をセットし、
//! `handle_strategy_run_system` が `errors.any() == true` のとき `TransportCommand` を送らないことを
//! `TransportCommandSender` の mpsc 受信端で確認する。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::replay::{ReplayStartupPhase, ReplayStartupProgress};
use backcast::trading::{
    LastRunResult, RunState, TradingSession, TransportCommand, TransportCommandSender,
};
use backcast::ui::components::{
    InstrumentRegistry, ScenarioMetadata, ScenarioStartupParams, ScenarioStartupParamsErrors,
    ScenarioWritebackPaths, StrategyRunRequested,
};
use backcast::ui::menu_bar::handle_strategy_run_system;
use backcast::ui::scenario_startup_panel::{
    commit_startup_params_to_scenario_system, ScenarioStartupParamCommit,
};

fn make_valid_scenario() -> ScenarioMetadata {
    ScenarioMetadata {
        schema_version: Some(2),
        instruments: vec!["7203.TSE".to_string()],
        start: Some("2025-01-06".to_string()),
        end: Some("2025-03-31".to_string()),
        granularity: Some("Daily".to_string()),
        initial_cash: Some(1_000_000),
    }
}

fn build_run_app(
    scenario: ScenarioMetadata,
) -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.init_resource::<Time<Real>>();
    app.insert_resource(scenario);
    app.insert_resource(InstrumentRegistry {
        ids: vec!["7203.TSE".to_string()],
        editable: true,
    });
    app.insert_resource(ScenarioWritebackPaths::default());
    app.init_resource::<ReplayStartupProgress>();
    app.init_resource::<ScenarioStartupParams>();
    app.insert_resource(TradingSession::default());
    app.insert_resource(LastRunResult::default());
    app.insert_resource(TransportCommandSender { tx });
    app.add_message::<StrategyRunRequested>();
    app.add_message::<ScenarioStartupParamCommit>();
    app.add_systems(
        Update,
        (
            commit_startup_params_to_scenario_system,
            handle_strategy_run_system,
        )
            .chain(),
    );

    (app, rx)
}

#[test]
fn j7_startup_panel_validation_blocks_run() {
    // ── ケース 1: 不正な Start 日付 → Run blocked ──
    {
        let (mut app, mut rx) = build_run_app(make_valid_scenario());
        // ScenarioWritebackPaths に cache sidecar パスを設定してパネルを有効化
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(std::path::PathBuf::from("/tmp/dummy_case1.json")),
        });

        // 不正な日付をコミット → errors.start がセットされる
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::Start("not-a-date".into()));
        app.update();

        // errors.start が Some であることを確認
        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            params.errors.start.is_some(),
            "不正な Start 日付で errors.start がセットされるはず"
        );

        // errors.any() == true の状態で Run を試みる
        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/strategy.py"),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "ケース1: errors.start あり → RunStrategy コマンドは送られないはず"
        );
        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible, "progress は visible にならないはず");
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);
    }

    // ── ケース 2: 不正な End 日付 ──
    {
        let (mut app, mut rx) = build_run_app(make_valid_scenario());
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(std::path::PathBuf::from("/tmp/dummy_case2.json")),
        });

        app.world_mut()
            .write_message(ScenarioStartupParamCommit::End("invalid".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(params.errors.end.is_some(), "不正な End 日付で errors.end がセットされるはず");

        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/strategy.py"),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "ケース2: errors.end あり → コマンドは送られないはず"
        );
    }

    // ── ケース 3: Start > End (cross-field error) ──
    {
        let (mut app, mut rx) = build_run_app(make_valid_scenario());
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(std::path::PathBuf::from("/tmp/dummy_case3.json")),
        });

        // 先に両フィールドを有効な値でコミット
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::Start("2025-03-31".into()));
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::End("2025-01-01".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            params.errors.cross_field.is_some(),
            "Start > End で cross_field エラーがセットされるはず"
        );

        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/strategy.py"),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "ケース3: cross_field エラーあり → コマンドは送られないはず"
        );
    }

    // ── ケース 4: 不正な Initial Cash ──
    {
        let (mut app, mut rx) = build_run_app(make_valid_scenario());
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(std::path::PathBuf::from("/tmp/dummy_case4.json")),
        });

        app.world_mut()
            .write_message(ScenarioStartupParamCommit::InitialCash("not-a-number".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            params.errors.initial_cash.is_some(),
            "不正な InitialCash で errors.initial_cash がセットされるはず"
        );

        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/strategy.py"),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "ケース4: errors.initial_cash あり → コマンドは送られないはず"
        );
    }

    // ── ケース 5: errors を直接注入 (granularity error) ──
    // commit_startup_params_to_scenario_system を経由せず errors を直接セットするパス
    {
        let (mut app, mut rx) = build_run_app(make_valid_scenario());

        app.world_mut()
            .resource_mut::<ScenarioStartupParams>()
            .errors = ScenarioStartupParamsErrors {
            granularity: Some("unknown granularity 'Tick'".to_string()),
            ..Default::default()
        };

        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/strategy.py"),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "ケース5: errors.granularity あり → コマンドは送られないはず"
        );
        let last_run = app.world().resource::<LastRunResult>();
        assert!(
            matches!(last_run.state, RunState::Idle),
            "RunState は Idle のままのはず"
        );
    }
}
