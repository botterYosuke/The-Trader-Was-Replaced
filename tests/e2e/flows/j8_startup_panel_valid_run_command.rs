//! J8 startup_panel_valid_run_command — Startup パネルの入力が有効なとき、`StrategyRunRequested` が
//! cache strategy path と scenario metadata を含む `TransportCommand::RunStrategy` を送ることを保証する（kind:ui）。
//!
//! `handle_strategy_run_system` は `startup_params.errors.any() == false` かつ
//! scenario に instruments/start/end/granularity が揃っているとき `RunStrategy` を送信する。
//! また `ReplayStartupProgress.visible = true` / `phase = CommandAccepted` に更新することも確認する。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::replay::{ReplayStartupPhase, ReplayStartupProgress};
use backcast::trading::{
    CurrentRun, RunState, TradingSession, TransportCommand, TransportCommandSender,
};
use backcast::ui::components::{
    InstrumentRegistry, ScenarioMetadata, ScenarioStartupParams, ScenarioWritebackPaths,
    StrategyRunRequested,
};
use backcast::ui::menu_bar::handle_strategy_run_system;

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

fn build_app(scenario: ScenarioMetadata) -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.init_resource::<Time<Real>>();
    app.insert_resource(scenario);
    // editable=false（instruments_ref 由来）にして Run 直前の inline sidecar flush を
    // 経由させない。flush は J16 / writeback 系で検証する別関心で、ここでは
    // 「有効 params → RunStrategy 送信」の gate だけを忠実に駆動する。
    app.insert_resource(InstrumentRegistry {
        ids: vec!["7203.TSE".to_string()],
        editable: false,
    });
    app.insert_resource(ScenarioWritebackPaths::default());
    app.init_resource::<ReplayStartupProgress>();
    app.init_resource::<ScenarioStartupParams>();
    app.insert_resource(TradingSession::default());
    app.insert_resource(CurrentRun::default());
    app.insert_resource(TransportCommandSender { tx });
    app.add_event::<StrategyRunRequested>();
    app.add_systems(Update, handle_strategy_run_system);

    (app, rx)
}

#[test]
fn j8_startup_panel_valid_run_command() {
    // ── ケース 1: 有効な params + 有効な scenario → RunStrategy が送られる ──
    {
        let (mut app, mut rx) = build_app(make_valid_scenario());

        let cache_path = std::path::PathBuf::from("/tmp/app_state.py");
        app.world_mut().send_event(StrategyRunRequested {
            cache_path: cache_path.clone(),
        });
        app.update();

        let cmd = rx
            .try_recv()
            .expect("有効な params で StrategyRunRequested → RunStrategy コマンドが送られるはず");

        match cmd {
            TransportCommand::RunStrategy {
                strategy_file,
                config,
                startup_id,
            } => {
                assert_eq!(
                    strategy_file, cache_path,
                    "strategy_file が StrategyRunRequested.cache_path と一致するはず"
                );
                assert_eq!(
                    config.instruments,
                    vec!["7203.TSE".to_string()],
                    "config.instruments が scenario から正しく取得されるはず"
                );
                assert_eq!(config.start, "2025-01-06");
                assert_eq!(config.end, "2025-03-31");
                assert_eq!(config.granularity, "Daily");
                assert_eq!(config.initial_cash, Some(1_000_000));
                assert_eq!(startup_id, 0, "最初の Run は startup_id=0 のはず");
            }
            other => panic!("想定外のコマンド: {:?}", other),
        }

        // progress が更新されること
        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(progress.visible, "RunStrategy 送信後 progress.visible が true になるはず");
        assert_eq!(
            progress.phase,
            ReplayStartupPhase::CommandAccepted,
            "progress.phase が CommandAccepted になるはず"
        );
        assert_eq!(progress.startup_id, 0);
        assert_eq!(progress.next_startup_id, 1);

        let current_run = app.world().resource::<CurrentRun>();
        assert!(
            matches!(current_run.state, RunState::Running),
            "RunState が Running になるはず"
        );
    }

    // ── ケース 2: 複数回 Run → startup_id がインクリメントされる ──
    {
        let (mut app, mut rx) = build_app(make_valid_scenario());

        // 1 回目
        app.world_mut().send_event(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/a.py"),
        });
        app.update();
        let cmd1 = rx.try_recv().unwrap();
        let id1 = match cmd1 {
            TransportCommand::RunStrategy { startup_id, .. } => startup_id,
            _ => panic!("RunStrategy expected"),
        };

        // progress.visible を false に戻してから 2 回目を送れるようにする
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = false;

        // 2 回目
        app.world_mut().send_event(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/a.py"),
        });
        app.update();
        let cmd2 = rx.try_recv().unwrap();
        let id2 = match cmd2 {
            TransportCommand::RunStrategy { startup_id, .. } => startup_id,
            _ => panic!("RunStrategy expected"),
        };

        assert_eq!(id1, 0);
        assert_eq!(id2, 1, "2 回目の startup_id は 1 になるはず");
    }

    // ── ケース 3: scenario に instruments がない → blocked ──
    {
        let empty = ScenarioMetadata {
            instruments: vec![],
            start: Some("2025-01-06".into()),
            end: Some("2025-03-31".into()),
            granularity: Some("Daily".into()),
            initial_cash: Some(1_000_000),
            ..Default::default()
        };
        let (mut app, mut rx) = build_app(empty);

        app.world_mut().send_event(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/a.py"),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "ケース3: instruments 空 → コマンドは送られないはず"
        );
        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
    }

    // ── ケース 4: TransportCommandSender が None (backend 未接続) → blocked ──
    {
        let mut app = App::new();
        app.init_resource::<Time<Real>>();
        app.insert_resource(make_valid_scenario());
        app.insert_resource(InstrumentRegistry {
            ids: vec!["7203.TSE".to_string()],
            editable: true,
        });
        app.insert_resource(ScenarioWritebackPaths::default());
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<ScenarioStartupParams>();
        app.insert_resource(TradingSession::default());
        app.insert_resource(CurrentRun::default());
        // TransportCommandSender を挿入しない
        app.add_event::<StrategyRunRequested>();
        app.add_systems(Update, handle_strategy_run_system);

        app.world_mut().send_event(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/a.py"),
        });
        app.update();

        // progress は更新されない（sender がないので送信失敗）
        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible, "ケース4: sender なし → progress.visible は false のはず");
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);
    }
}
