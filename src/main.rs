use backcast::backend_supervisor::{
    BackendLifecycle, BackendLifecycleHandle, BackendSupervisorPlugin, SupervisorCommand,
    SupervisorCommandSender, SupervisorTaskSeed, run_supervisor,
};
use backcast::backend_sync::{
    BackendEventChannel, StatusUpdateChannel, backend_event_drain_system,
    backend_restart_resync_system,
    request_force_account_snapshot_on_live_entry, request_get_orders_on_venue_connected,
    status_update_system,
};
use backcast::backend_transport::{BackendTransport, GrpcTransport, InProcTransport};
use backcast::camera::{pancam_suppression_over_editor_system, setup_camera};
use backcast::grid::GridPlugin;
use backcast::replay::ReplayStartupProgress;
use backcast::trading::{
    AvailableInstruments, BackendChannel, BackendStatus,
    CurrentRun, ExecutionModeRes, LastPrices, LiveOrders,
    OrderFeedback, PortfolioState,
    ReconcilePrompt, ReloginPrompt, ReplaySpeed, SecretPrompt, SelectedSymbol, Tickers,
    TradingSettings, TransportCommand, TransportCommandSender, VenueStatusRes,
    backend_update_system,
};
use backcast::ui::UiPlugin;
use backcast::ui::run_result_panel::{
    auto_hide_startup_progress_system, replay_startup_timeout_system,
};
use bevy::prelude::*;
use bevy_pancam::{PanCamPlugin, PanCamSystems};
use tokio::sync::mpsc;

// Bevy's compute task pool threads don't inherit the Tokio runtime context,
// so we capture the handle here (before App::run takes over) and pass it as a resource.
#[derive(Resource, Clone)]
struct TokioHandle(tokio::runtime::Handle);

#[tokio::main]
async fn main() {
    let tokio_handle = TokioHandle(tokio::runtime::Handle::current());
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Trader Dashboard - Premium Infinite Canvas".to_string(),
                ..default()
            }),
            ..default()
        }))
        // Idle CPU 削減: focused 5fps / unfocused 0.5fps の reactive update。
        // backend からの mpsc push は最大 200ms 遅延で UI に反映される
        // (desktop_app() の 5s は trading UI には粗すぎたため短縮)。
        .insert_resource(bevy::winit::WinitSettings {
            focused_mode: bevy::winit::UpdateMode::reactive(std::time::Duration::from_millis(200)),
            unfocused_mode: bevy::winit::UpdateMode::reactive_low_power(
                std::time::Duration::from_secs(2),
            ),
        })
        .add_plugins(PanCamPlugin)
        .add_plugins(UiPlugin)
        .add_plugins(GridPlugin)
        .add_plugins(BackendSupervisorPlugin)
        .insert_resource(backcast::trading::InstrumentTradingDataMap::default())
        .insert_resource(backcast::trading::TradingSession::default())
        .insert_resource(TradingSettings::default())
        .insert_resource(BackendStatus::default())
        .insert_resource(CurrentRun::default())
        .init_resource::<ReplayStartupProgress>()
        .insert_resource(AvailableInstruments::default())
        .insert_resource(PortfolioState::default())
        .insert_resource(ReplaySpeed::default())
        .insert_resource(VenueStatusRes::default())
        .insert_resource(ExecutionModeRes::default())
        .insert_resource(Tickers::default())
        .insert_resource(LastPrices::default())
        .insert_resource(SelectedSymbol::default())
        // Phase 9 §3.2 / §3.10: Live order book + active SecretRequired prompt.
        // Initialized here (not in UiPlugin) because the transport-facing
        // `status_update_system` / `backend_event_drain_system` that mutate them
        // live in this binary.
        .insert_resource(LiveOrders::default())
        .insert_resource(SecretPrompt::default())
        .insert_resource(ReloginPrompt::default())
        .insert_resource(ReconcilePrompt::default())
        .insert_resource(OrderFeedback::default())
        .insert_resource(tokio_handle)
        .add_systems(
            Startup,
            (
                setup_camera,
                setup_backend_connection,
                spawn_supervisor_task_system,
            ),
        )
        .add_systems(
            Update,
            (
                backend_update_system,
                status_update_system,
                // Run the event drain (which sets SecretPrompt.active on
                // SecretRequired) before the secret modal's keyboard-drain, so on
                // the frame the prompt opens the modal — not the picker/menu —
                // consumes that frame's keystrokes (Round 1 bevy-ecs H1).
                backend_event_drain_system
                    .before(backcast::ui::secret_modal::secret_modal_input_system),
                // Phase 9 §3.8: after an auto-restart reaches Ready, fire GetOrders
                // to reconcile the optimistic order list with the fresh backend.
                backend_restart_resync_system,
                // Issue #29 Slice 2' (Step 5): Live entry 検出で ForceAccountSnapshot を撃つ。
                // status_update_system が exec_mode を確定させた後に読む（race-free）。
                request_force_account_snapshot_on_live_entry.after(status_update_system),
                // Issue #29 Slice 3b: venue CONNECTED 時に GetOrders を撃って接続前の
                // working-orders を seed する（status_update_system が venue_state を確定後）。
                request_get_orders_on_venue_connected.after(status_update_system),
                auto_hide_startup_progress_system.after(status_update_system),
                replay_startup_timeout_system,
                // PanCam の do_camera_zoom より前に走らせ、enabled フラグを先に確定させる。
                pancam_suppression_over_editor_system.before(PanCamSystems),
            ),
        )
        .add_systems(Last, app_exit_shutdown_system)
        .run();
}

fn spawn_supervisor_task_system(tokio: Res<TokioHandle>, mut seed: ResMut<SupervisorTaskSeed>) {
    if let Some((config, lifecycle_tx, cmd_rx, ownership_tx)) = seed.inner.take() {
        tokio
            .0
            .spawn(run_supervisor(config, lifecycle_tx, cmd_rx, ownership_tx));
    }
}

/// On `AppExit`, ask the supervisor to gracefully shut down the backend and
/// block the main thread (up to 2.5s) until it acks.
fn should_send_graceful_shutdown(lifecycle: BackendLifecycle) -> bool {
    !matches!(
        lifecycle,
        BackendLifecycle::Disabled
            | BackendLifecycle::Stopped
            | BackendLifecycle::Crashed
            | BackendLifecycle::StartupFailed(_)
    )
}

fn app_exit_shutdown_system(
    mut app_exit: MessageReader<AppExit>,
    cmd_sender: Res<SupervisorCommandSender>,
    lifecycle: Res<BackendLifecycleHandle>,
) {
    if app_exit.read().next().is_none() {
        return;
    }
    if !should_send_graceful_shutdown(lifecycle.current()) {
        return;
    }

    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
    if cmd_sender
        .tx
        .send(SupervisorCommand::Shutdown {
            grace_seconds: 0,
            reply_tx: Some(tx),
        })
        .is_err()
    {
        warn!("[backend] AppExit: supervisor command channel closed; skipping graceful shutdown");
        return;
    }

    match rx.recv_timeout(std::time::Duration::from_millis(2500)) {
        Ok(()) => info!("[backend] AppExit: graceful shutdown acked"),
        Err(_) => warn!(
            "[backend] AppExit: shutdown ack timed out after 2.5s; exiting anyway (child may be orphaned)"
        ),
    }
}

fn setup_backend_connection(
    mut commands: Commands,
    settings: Res<TradingSettings>,
    tokio_handle: Res<TokioHandle>,
    lifecycle_handle: Res<BackendLifecycleHandle>,
) {
    let (state_tx, state_rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendChannel { rx: state_rx });

    let (status_tx, status_rx) = mpsc::unbounded_channel();
    commands.insert_resource(StatusUpdateChannel { rx: status_rx });

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendEventChannel { rx: event_rx });

    // Transport command channel: sender lives as a Bevy resource, receiver moves into the tokio task.
    let (transport_tx, transport_rx) = mpsc::unbounded_channel::<TransportCommand>();
    commands.insert_resource(TransportCommandSender { tx: transport_tx });

    if !settings.backend_enabled {
        info!("Backend connection is disabled. Running in simulation mode.");
        // transport_rx is dropped here; sends from UI will silently fail — that's fine.
        return;
    }

    info!(
        "Backend connection is enabled. Connecting to {}...",
        settings.backend_url
    );

    let transport: Box<dyn BackendTransport> = if settings.use_inproc {
        info!("InProc transport selected (BACKEND_TRANSPORT=inproc).");
        Box::new(InProcTransport {
            catalog_path: settings.catalog_path.clone(),
            max_history_len: settings.max_history_points,
            python_engine_path: settings.python_engine_path.clone(),
            poll_interval_ms: settings.poll_interval_ms,
            live_venue_id: settings.live_venue_id.clone(),
        })
    } else {
        Box::new(GrpcTransport {
            url: settings.backend_url.clone(),
            token: settings.token.clone(),
            poll_interval_ms: settings.poll_interval_ms,
            catalog_path: settings.catalog_path.clone(),
        })
    };
    let handle = tokio_handle.0.clone();
    handle.spawn(transport.run(
        transport_rx,
        state_tx,
        status_tx,
        event_tx,
        lifecycle_handle.subscribe(),
    ));
}

#[cfg(test)]
mod tests {
    use super::{BackendLifecycle, ReplayStartupProgress, should_send_graceful_shutdown};
    use backcast::backend_sync::{
        apply_account_event, apply_available_failed, apply_available_loaded, apply_status_update,
    };
    use backcast::replay::ReplayStartupPhase;
    use backcast::trading::{
        AccountPosition, AvailableInstruments, BackendStartupStage, BackendStatus,
        BackendStatusUpdate, CurrentRun, ExecutionModeRes, LastPrices, LiveOrders,
        OrderFeedback, PortfolioState, ReconcilePrompt, RunState, SecretPrompt,
        Ticker, Tickers, VenueStatusRes,
    };
    use chrono::NaiveDate;

    #[test]
    fn account_event_reduces_into_portfolio_with_derived_equity() {
        let mut portfolio = PortfolioState::default();
        apply_account_event(
            &mut portfolio,
            100_000.0, // cash
            250_000.0, // buying_power
            vec![
                AccountPosition {
                    symbol: "7203.T".to_string(),
                    qty: 100,
                    avg_price: 2500.0,
                    unrealized_pnl: 1000.0,
                },
                AccountPosition {
                    symbol: "9984.T".to_string(),
                    qty: 50,
                    avg_price: 8000.0,
                    unrealized_pnl: -2000.0,
                },
            ],
        );
        assert_eq!(portfolio.cash, 100_000.0);
        assert_eq!(portfolio.buying_power, 250_000.0);
        assert_eq!(portfolio.positions.len(), 2);
        assert!(portfolio.loaded, "AccountEvent marks the portfolio loaded");
        // equity = cash + Σ(qty*avg_price + unrealized_pnl)
        //        = 100_000 + (100*2500 + 1000) + (50*8000 - 2000)
        //        = 100_000 + 251_000 + 398_000 = 749_000
        assert_eq!(portfolio.equity, 749_000.0);
        assert_eq!(portfolio.positions[0].symbol, "7203.T");
        assert_eq!(portfolio.positions[0].qty, 100);
        assert_eq!(portfolio.positions[1].unrealized_pnl, -2000.0);
    }

    #[test]
    fn account_event_with_no_positions_sets_equity_to_cash() {
        let mut portfolio = PortfolioState::default();
        apply_account_event(&mut portfolio, 500_000.0, 1_000_000.0, vec![]);
        assert_eq!(portfolio.equity, 500_000.0);
        assert!(portfolio.positions.is_empty());
        assert!(portfolio.loaded);
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn fresh_progress(startup_id: u64, visible: bool) -> ReplayStartupProgress {
        ReplayStartupProgress {
            visible,
            startup_id,
            ..ReplayStartupProgress::default()
        }
    }

    fn apply(update: BackendStatusUpdate, progress: &mut ReplayStartupProgress) -> CurrentRun {
        let mut status = BackendStatus::default();
        let mut current_run = CurrentRun::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut tickers = Tickers::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        let mut reconcile_prompt = ReconcilePrompt::default();
        apply_status_update(
            update,
            &mut status,
            &mut current_run,
            &mut portfolio,
            &mut available,
            progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
            &mut reconcile_prompt,
            &mut SecretPrompt::default(),
        );
        current_run
    }

    /// Variant of `apply` that seeds an initial `OrderFeedback` and returns it,
    /// for verifying the order-notice set/clear behavior (Round 2 M-A/M-B).
    fn apply_feedback(update: BackendStatusUpdate, initial: Option<&str>) -> OrderFeedback {
        let mut status = BackendStatus::default();
        let mut current_run = CurrentRun::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut progress = ReplayStartupProgress::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut tickers = Tickers::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback {
            message: initial.map(str::to_string),
        };
        let mut reconcile_prompt = ReconcilePrompt::default();
        apply_status_update(
            update,
            &mut status,
            &mut current_run,
            &mut portfolio,
            &mut available,
            &mut progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
            &mut reconcile_prompt,
            &mut SecretPrompt::default(),
        );
        order_feedback
    }

    #[test]
    fn order_rejected_sets_feedback_message() {
        let fb = apply_feedback(
            BackendStatusUpdate::OrderRejected {
                action: "発注".to_string(),
                error_code: "EXECUTION_MODE_PRECONDITION".to_string(),
            },
            None,
        );
        assert_eq!(
            fb.message.as_deref(),
            Some("発注が拒否されました (EXECUTION_MODE_PRECONDITION)")
        );
    }

    /// §3.10 / §2.2 regression: `OrderNotice` must surface verbatim in the
    /// OrderPanel feedback line.
    #[test]
    fn order_notice_surfaces_in_order_feedback() {
        let fb = apply_feedback(
            BackendStatusUpdate::OrderNotice {
                message: "発注応答が不完全です — venue で注文状態を確認してください".to_string(),
            },
            None,
        );
        assert_eq!(
            fb.message.as_deref(),
            Some("発注応答が不完全です — venue で注文状態を確認してください"),
            "incomplete-success / transport-error notices must reach the trader"
        );

        let fb2 = apply_feedback(
            BackendStatusUpdate::OrderNotice {
                message: "通信エラー — venue で注文状態を確認してください (発注)".to_string(),
            },
            None,
        );
        assert!(
            fb2.message
                .as_deref()
                .is_some_and(|m| m.contains("通信エラー")),
            "transport-error notice must reach the trader"
        );
    }

    #[test]
    fn order_seeded_clears_stale_feedback() {
        let fb = apply_feedback(
            BackendStatusUpdate::OrderSeeded {
                client_order_id: "c1".to_string(),
                venue_order_id: "v1".to_string(),
                symbol: "7203.T".to_string(),
                side: "BUY".to_string(),
                qty: 100.0,
                price: None,
                status: "ACCEPTED".to_string(),
                filled_qty: 0.0,
                avg_price: 0.0,
                ts_ms: 1,
                strategy_id: "MANUAL-001".to_string(),
            },
            Some("発注が拒否されました (X)"),
        );
        assert!(
            fb.message.is_none(),
            "a successful place must clear the stale reject notice"
        );
    }

    #[test]
    fn mode_change_clears_stale_feedback() {
        let fb = apply_feedback(
            BackendStatusUpdate::ExecutionModeChanged {
                mode: backcast::trading::ExecutionMode::Replay,
            },
            Some("発注が拒否されました (X)"),
        );
        assert!(
            fb.message.is_none(),
            "switching execution mode must drop the prior-context notice"
        );
    }

    #[test]
    fn mode_change_resets_portfolio_to_prevent_live_replay_bleed() {
        use backcast::trading::ExecutionMode;
        let mut status = BackendStatus::default();
        let mut current_run = CurrentRun::default();
        let mut portfolio = PortfolioState {
            cash: 100_000.0,
            buying_power: 250_000.0,
            equity: 749_000.0,
            loaded: true,
            ..Default::default()
        };
        let mut available = AvailableInstruments::default();
        let mut progress = ReplayStartupProgress::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        };
        let mut tickers = Tickers::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        let mut reconcile_prompt = ReconcilePrompt::default();
        apply_status_update(
            BackendStatusUpdate::ExecutionModeChanged {
                mode: ExecutionMode::Replay,
            },
            &mut status,
            &mut current_run,
            &mut portfolio,
            &mut available,
            &mut progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
            &mut reconcile_prompt,
            &mut SecretPrompt::default(),
        );
        assert!(
            !portfolio.loaded,
            "stale Live account snapshot must not bleed into the Replay view"
        );
        assert_eq!(portfolio.cash, 0.0);
        assert_eq!(portfolio.equity, 0.0);
        assert_eq!(portfolio.buying_power, 0.0);
    }

    /// Variant of `apply` that returns the `Tickers` resource after the update.
    fn apply_with_tickers(
        update: BackendStatusUpdate,
        progress: &mut ReplayStartupProgress,
        tickers: &mut Tickers,
    ) {
        let mut status = BackendStatus::default();
        let mut current_run = CurrentRun::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        let mut reconcile_prompt = ReconcilePrompt::default();
        apply_status_update(
            update,
            &mut status,
            &mut current_run,
            &mut portfolio,
            &mut available,
            progress,
            &mut venue_status,
            &mut exec_mode,
            tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
            &mut reconcile_prompt,
            &mut SecretPrompt::default(),
        );
    }

    #[test]
    fn graceful_shutdown_only_for_live_states() {
        assert!(!should_send_graceful_shutdown(BackendLifecycle::Disabled));
        assert!(!should_send_graceful_shutdown(BackendLifecycle::Stopped));
        assert!(!should_send_graceful_shutdown(BackendLifecycle::Crashed));
        assert!(!should_send_graceful_shutdown(
            BackendLifecycle::StartupFailed("BACKEND_NOT_REACHABLE")
        ));

        assert!(should_send_graceful_shutdown(BackendLifecycle::NotStarted));
        assert!(should_send_graceful_shutdown(
            BackendLifecycle::ProbingExisting
        ));
        assert!(should_send_graceful_shutdown(BackendLifecycle::Spawning));
        assert!(should_send_graceful_shutdown(BackendLifecycle::Ready));
        assert!(should_send_graceful_shutdown(
            BackendLifecycle::ShuttingDown
        ));
    }

    #[test]
    fn test_available_loaded_clears_in_flight() {
        let mut av = AvailableInstruments::default();
        let date = d("2025-01-15");
        av.in_flight.insert(date);
        apply_available_loaded(&mut av, date, vec!["7203".into(), "9984".into()]);
        assert!(!av.in_flight.contains(&date), "in_flight must be cleared");
        assert_eq!(
            av.by_end_date.get(&date).map(Vec::as_slice),
            Some(["7203".to_string(), "9984".to_string()].as_slice()),
        );
    }

    #[test]
    fn test_available_failed_sets_last_error() {
        let mut av = AvailableInstruments::default();
        let date = d("2025-01-15");
        av.in_flight.insert(date);
        apply_available_failed(&mut av, date, "grpc unavailable".into());
        assert!(
            !av.in_flight.contains(&date),
            "in_flight must be cleared on failure too"
        );
        assert_eq!(av.last_error, Some((date, "grpc unavailable".into())));
        assert!(
            !av.by_end_date.contains_key(&date),
            "cache must not be populated on failure"
        );
    }

    #[test]
    fn test_available_loaded_overwrites_existing() {
        let mut av = AvailableInstruments::default();
        let date = d("2025-01-15");
        av.by_end_date.insert(date, vec!["OLD".into()]);
        av.in_flight.insert(date);
        apply_available_loaded(&mut av, date, vec!["NEW1".into(), "NEW2".into()]);
        assert_eq!(
            av.by_end_date.get(&date).map(Vec::as_slice),
            Some(["NEW1".to_string(), "NEW2".to_string()].as_slice()),
        );
        assert!(!av.in_flight.contains(&date));
    }

    // --- ReplayStartup arm (#3, #3b, #3c, #3d) ---

    #[test]
    fn apply_replay_startup_phase_when_matching() {
        let mut progress = fresh_progress(7, true);
        apply(
            BackendStatusUpdate::ReplayStartup {
                startup_id: 7,
                stage: BackendStartupStage::LoadingData,
            },
            &mut progress,
        );
        assert_eq!(progress.phase, ReplayStartupPhase::LoadingData);
        assert!(!progress.start_engine_accepted);
    }

    #[test]
    fn apply_replay_startup_ignored_when_not_visible() {
        let mut progress = fresh_progress(7, false);
        apply(
            BackendStatusUpdate::ReplayStartup {
                startup_id: 7,
                stage: BackendStartupStage::LoadingData,
            },
            &mut progress,
        );
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);
    }

    #[test]
    fn apply_replay_startup_ignored_when_startup_id_mismatch() {
        let mut progress = fresh_progress(7, true);
        apply(
            BackendStatusUpdate::ReplayStartup {
                startup_id: 9,
                stage: BackendStartupStage::LoadingData,
            },
            &mut progress,
        );
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);
    }

    #[test]
    fn apply_replay_startup_waiting_sets_start_engine_accepted() {
        let mut progress = fresh_progress(7, true);
        apply(
            BackendStatusUpdate::ReplayStartup {
                startup_id: 7,
                stage: BackendStartupStage::WaitingForFirstTick,
            },
            &mut progress,
        );
        assert_eq!(progress.phase, ReplayStartupPhase::WaitingForFirstTick);
        assert!(progress.start_engine_accepted);
    }

    // --- RunFailed arm (#4, #4b, #4c) ---

    #[test]
    fn apply_run_failed_sets_progress_error_when_matching() {
        let mut progress = fresh_progress(7, true);
        let current_run = apply(
            BackendStatusUpdate::RunFailed {
                startup_id: Some(7),
                error: "boom".into(),
            },
            &mut progress,
        );
        assert_eq!(progress.error.as_deref(), Some("boom"));
        assert_eq!(
            current_run.state,
            RunState::Failed {
                error: "boom".into()
            }
        );
    }

    #[test]
    fn apply_run_failed_with_none_startup_id_only_updates_current_run() {
        let mut progress = fresh_progress(7, true);
        let current_run = apply(
            BackendStatusUpdate::RunFailed {
                startup_id: None,
                error: "boom".into(),
            },
            &mut progress,
        );
        assert!(progress.error.is_none());
        assert_eq!(
            current_run.state,
            RunState::Failed {
                error: "boom".into()
            }
        );
    }

    #[test]
    fn apply_run_failed_with_mismatched_startup_id_ignored() {
        let mut progress = fresh_progress(7, true);
        let current_run = apply(
            BackendStatusUpdate::RunFailed {
                startup_id: Some(9),
                error: "boom".into(),
            },
            &mut progress,
        );
        assert!(progress.error.is_none());
        assert_eq!(
            current_run.state,
            RunState::Failed {
                error: "boom".into()
            }
        );
    }

    // --- RunComplete arm (#7, #7b) ---

    #[test]
    fn apply_run_complete_resets_progress_when_matching() {
        let mut progress = ReplayStartupProgress {
            visible: true,
            startup_id: 7,
            phase: ReplayStartupPhase::WaitingForFirstTick,
            detail: Some("loading".into()),
            baseline_timestamp_ms: Some(1234),
            started_at_elapsed: Some(std::time::Duration::from_secs(1)),
            start_engine_accepted: true,
            ..ReplayStartupProgress::default()
        };
        let current_run = apply(
            BackendStatusUpdate::RunComplete {
                startup_id: Some(7),
                run_id: "r1".into(),
                summary_json: "{}".into(),
            },
            &mut progress,
        );
        assert!(!progress.visible);
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);
        assert!(progress.detail.is_none());
        assert!(progress.baseline_timestamp_ms.is_none());
        assert!(progress.started_at_elapsed.is_none());
        assert!(!progress.start_engine_accepted);
        assert!(matches!(current_run.state, RunState::Completed { .. }));
    }

    #[test]
    fn apply_run_complete_with_none_startup_id_keeps_progress() {
        let mut progress = ReplayStartupProgress {
            visible: true,
            startup_id: 7,
            phase: ReplayStartupPhase::WaitingForFirstTick,
            start_engine_accepted: true,
            ..ReplayStartupProgress::default()
        };
        apply(
            BackendStatusUpdate::RunComplete {
                startup_id: None,
                run_id: "r1".into(),
                summary_json: "{}".into(),
            },
            &mut progress,
        );
        assert!(
            progress.visible,
            "RunComplete with no startup_id must not close the new window"
        );
        assert_eq!(progress.phase, ReplayStartupPhase::WaitingForFirstTick);
    }

    #[test]
    fn apply_run_complete_with_stale_startup_id_keeps_progress() {
        let mut progress = ReplayStartupProgress {
            visible: true,
            startup_id: 7,
            phase: ReplayStartupPhase::WaitingForFirstTick,
            start_engine_accepted: true,
            ..ReplayStartupProgress::default()
        };
        apply(
            BackendStatusUpdate::RunComplete {
                startup_id: Some(9),
                run_id: "r1".into(),
                summary_json: "{}".into(),
            },
            &mut progress,
        );
        assert!(progress.visible);
        assert_eq!(progress.phase, ReplayStartupPhase::WaitingForFirstTick);
        assert!(progress.start_engine_accepted);
    }

    // --- InstrumentsListed arm (Phase 8 §3.5 / Phase 8.7 §3.3 D6c) ---

    fn t(id: &str) -> Ticker {
        Ticker {
            id: id.into(),
            name: id.into(),
            market: String::new(),
        }
    }

    #[test]
    fn apply_instruments_listed_overwrites_tickers() {
        use backcast::trading::{TickersSource, TickersStatus};
        let mut progress = ReplayStartupProgress::default();
        let mut tickers = Tickers {
            list: vec![t("OLD.TSE")],
            ..Tickers::default()
        };
        apply_with_tickers(
            BackendStatusUpdate::InstrumentsListed {
                source: TickersSource::ReplayCatalogFallback,
                instruments: vec![t("1301.TSE"), t("7203.TSE")],
            },
            &mut progress,
            &mut tickers,
        );
        assert_eq!(
            tickers
                .list
                .iter()
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1301.TSE", "7203.TSE"],
            "InstrumentsListed must replace (not merge with) the prior universe",
        );
        assert_eq!(tickers.source, TickersSource::ReplayCatalogFallback);
        assert_eq!(tickers.status, TickersStatus::Loaded);
    }

    #[test]
    fn apply_instruments_listed_empty_clears_tickers() {
        use backcast::trading::TickersSource;
        let mut progress = ReplayStartupProgress::default();
        let mut tickers = Tickers {
            list: vec![t("1301.TSE")],
            ..Tickers::default()
        };
        apply_with_tickers(
            BackendStatusUpdate::InstrumentsListed {
                source: TickersSource::LiveVenue,
                instruments: vec![],
            },
            &mut progress,
            &mut tickers,
        );
        assert!(tickers.list.is_empty());
    }

    // --- LastPricesUpdated arm (Phase 8 §3.5 sidebar last-price column) ---

    fn apply_with_last_prices(
        update: BackendStatusUpdate,
        progress: &mut ReplayStartupProgress,
        last_prices: &mut LastPrices,
    ) {
        let mut status = BackendStatus::default();
        let mut current_run = CurrentRun::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut tickers = Tickers::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        let mut reconcile_prompt = ReconcilePrompt::default();
        apply_status_update(
            update,
            &mut status,
            &mut current_run,
            &mut portfolio,
            &mut available,
            progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            last_prices,
            &mut live_orders,
            &mut order_feedback,
            &mut reconcile_prompt,
            &mut SecretPrompt::default(),
        );
    }

    #[test]
    fn apply_last_prices_updated_overwrites_resource() {
        use std::collections::HashMap;
        let mut progress = ReplayStartupProgress::default();
        let mut last_prices = LastPrices {
            map: HashMap::from([
                ("7203".to_string(), 100.0_f64),
                ("9999".to_string(), 1.0_f64),
            ]),
        };
        let new_prices = HashMap::from([
            ("7203".to_string(), 101.0_f64),
            ("8306".to_string(), 500.0_f64),
        ]);
        apply_with_last_prices(
            BackendStatusUpdate::LastPricesUpdated {
                prices: new_prices.clone(),
            },
            &mut progress,
            &mut last_prices,
        );
        assert_eq!(
            last_prices.map, new_prices,
            "LastPricesUpdated must replace (not merge with) the prior map",
        );
    }

    #[test]
    fn apply_last_prices_updated_empty_clears_resource() {
        use std::collections::HashMap;
        let mut progress = ReplayStartupProgress::default();
        let mut last_prices = LastPrices {
            map: HashMap::from([("7203".to_string(), 100.0_f64)]),
        };
        apply_with_last_prices(
            BackendStatusUpdate::LastPricesUpdated {
                prices: HashMap::new(),
            },
            &mut progress,
            &mut last_prices,
        );
        assert!(
            last_prices.map.is_empty(),
            "empty LastPricesUpdated (e.g. Replay mode) must clear the resource",
        );
    }
}
