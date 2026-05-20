use backcast::backend_supervisor::{
    BackendLifecycle, BackendLifecycleHandle, BackendSupervisorPlugin, SupervisorCommand,
    SupervisorCommandSender, SupervisorTaskSeed, run_supervisor,
};
use backcast::backend_sync::{
    BackendEventChannel, StatusUpdateChannel, backend_event_drain_system, status_update_system,
};
use backcast::camera::{pancam_suppression_over_editor_system, setup_camera};
use backcast::grid::GridPlugin;
use backcast::replay::ReplayStartupProgress;
use backcast::trading::{
    AvailableInstruments, BackendChannel, BackendStartupStage, BackendStatus, BackendStatusUpdate,
    ExecutionMode, ExecutionModeRes, LastPrices, LastRunResult, LiveOrders, OrderFeedback,
    PortfolioOrder, PortfolioPosition, PortfolioState, ReplaySpeed, SecretPrompt, SelectedSymbol,
    Ticker, Tickers, TickersSource, TradingSettings, TransportCommand, TransportCommandSender,
    VenueState, VenueStatusRes, backend_update_system, engine, tickers_source_to_wire,
};
use backcast::ui::UiPlugin;
use backcast::ui::replay_startup_window::{
    animate_replay_startup_bar_system, auto_hide_replay_startup_window_system,
    replay_startup_close_button_system, replay_startup_timeout_system, spawn_replay_startup_window,
    update_replay_startup_window_system,
};
use bevy::prelude::*;
use bevy_pancam::{PanCamPlugin, PanCamSystemSet};
use engine::data_engine_client::DataEngineClient;
use engine::{
    EngineKind, EngineStartConfig, ForceStopReplayRequest, GetPortfolioRequest, GetStateRequest,
    ListAllListedSymbolsRequest, ListInstrumentsRequest, LoadReplayDataRequest, PauseReplayRequest,
    ReplayGranularity, ResumeReplayRequest, SetExecutionModeRequest, SetReplaySpeedRequest,
    StartEngineRequest, StartEngineResponse, StepReplayRequest, SubscribeBackendEventsReq,
    SubscribeRequest, VenueLoginRequest, VenueLogoutRequest,
};
use tokio::sync::mpsc;

// Bevy's compute task pool threads don't inherit the Tokio runtime context,
// so we capture the handle here (before App::run takes over) and pass it as a resource.
#[derive(Resource, Clone)]
struct TokioHandle(tokio::runtime::Handle);

fn parse_replay_granularity(s: &str) -> Result<i32, String> {
    match s {
        "Daily" => Ok(ReplayGranularity::Daily as i32),
        "Minute" => Ok(ReplayGranularity::Minute as i32),
        other => Err(format!("unknown granularity: {:?}", other)),
    }
}

/// Parse `VenueState` from backend string (e.g. `"CONNECTED"`).
/// Returns `None` for unknown values; caller should warn and skip.
fn parse_venue_state(s: &str) -> Option<VenueState> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

/// Parse `ExecutionMode` from backend string (e.g. `"LiveManual"`).
/// Returns `None` for unknown values; caller should warn and skip.
fn parse_execution_mode(s: &str) -> Option<ExecutionMode> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

/// Fire `ListInstruments` off the main polling loop and emit the three-part
/// `InstrumentsListStarted` / `InstrumentsListed` / `InstrumentsListFailed`
/// sequence. Used at startup and on venue state transitions; the work runs
/// on a separate task so the poll cadence is not gated on backend list latency.
fn fire_list_instruments(
    client: &DataEngineClient<tonic::transport::Channel>,
    token: &str,
    source: TickersSource,
    status_tx: &mpsc::UnboundedSender<BackendStatusUpdate>,
) {
    let mut li_client = client.clone();
    let li_token = token.to_owned();
    let li_status_tx = status_tx.clone();
    let wire_source = tickers_source_to_wire(source);
    // Emit InFlight immediately before spawning so the sidebar can show a spinner.
    let _ = li_status_tx.send(BackendStatusUpdate::InstrumentsListStarted { source });
    tokio::spawn(async move {
        let req = tonic::Request::new(ListInstrumentsRequest {
            token: li_token,
            source: wire_source,
        });
        match li_client.list_instruments(req).await {
            Ok(resp) => {
                let inner = resp.into_inner();
                if inner.success {
                    let instruments: Vec<Ticker> = inner
                        .instruments
                        .into_iter()
                        .map(|i| Ticker {
                            id: i.id,
                            name: i.name,
                            market: i.market,
                        })
                        .collect();
                    info!(
                        "ListInstruments(auto) ok: {} instruments",
                        instruments.len()
                    );
                    let _ = li_status_tx.send(BackendStatusUpdate::InstrumentsListed {
                        source,
                        instruments,
                    });
                } else {
                    warn!(
                        "ListInstruments(auto) returned !success: {}",
                        inner.error_message
                    );
                    let _ = li_status_tx.send(BackendStatusUpdate::InstrumentsListFailed {
                        source,
                        error: inner.error_message,
                    });
                }
            }
            Err(e) => {
                error!("ListInstruments(auto) failed: {}", e);
                let _ = li_status_tx.send(BackendStatusUpdate::InstrumentsListFailed {
                    source,
                    error: e.to_string(),
                });
            }
        }
    });
}

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
        .insert_resource(LastRunResult::default())
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
        .insert_resource(OrderFeedback::default())
        .insert_resource(tokio_handle)
        .add_systems(
            Startup,
            (
                setup_camera,
                setup_backend_connection,
                spawn_supervisor_task_system,
                spawn_replay_startup_window,
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
                update_replay_startup_window_system,
                animate_replay_startup_bar_system,
                auto_hide_replay_startup_window_system.after(status_update_system),
                replay_startup_close_button_system,
                replay_startup_timeout_system,
                // PanCam の do_camera_zoom より前に走らせ、enabled フラグを先に確定させる。
                pancam_suppression_over_editor_system.before(PanCamSystemSet),
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
/// block the main thread (up to 2.5s) until it acks. The supervisor decides
/// whether to actually fire the Shutdown RPC based on `own_process` (C-8), so
/// this system always sends the command regardless of ownership. Runs in `Last`
/// so it observes the `AppExit` raised earlier this frame (by `exit_on_all_closed`
/// or a manual quit) before the winit runner checks `should_exit()`.
/// Decide whether `AppExit` cleanup should fire a graceful `Shutdown` to the
/// supervisor. We skip states where the backend isn't running (or never
/// started): the supervisor task may have dropped its cmd_rx, so a Shutdown
/// send would never be acked — sending would just burn the 2.5s timeout.
fn should_send_graceful_shutdown(lifecycle: BackendLifecycle) -> bool {
    !matches!(
        lifecycle,
        BackendLifecycle::Disabled
            | BackendLifecycle::Stopped
            | BackendLifecycle::Crashed
            | BackendLifecycle::StartupFailed(_)
    )
}

/// Backend-events reconnect backoff: wait for either a lifecycle change or a
/// 500ms timer before retrying the stream. A streaming RPC can end (or a
/// connect/subscribe can transiently fail) without the lifecycle moving, so
/// blocking on `changed()` alone would stall the transport indefinitely; the
/// timer bounds the wait so the loop self-heals. Returns `false` when the
/// supervisor's watch sender was dropped (app exit) — the caller should return.
async fn events_reconnect_backoff(rx: &mut tokio::sync::watch::Receiver<BackendLifecycle>) -> bool {
    tokio::select! {
        changed = rx.changed() => changed.is_ok(),
        _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => true,
    }
}

fn app_exit_shutdown_system(
    mut app_exit: EventReader<AppExit>,
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
    let (tx, rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendChannel { rx });

    let (status_tx, status_rx) = mpsc::unbounded_channel();
    commands.insert_resource(StatusUpdateChannel { rx: status_rx });

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendEventChannel { rx: event_rx });

    // Transport command channel: sender lives as a Bevy resource, receiver moves into the tokio task.
    let (transport_tx, mut transport_rx) = mpsc::unbounded_channel::<TransportCommand>();
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

    let url = settings.backend_url.clone();
    let token = settings.token.clone();
    let interval = settings.poll_interval_ms;
    let catalog_path = settings.catalog_path.clone();

    let handle = tokio_handle.0.clone();
    let mut lifecycle_rx = lifecycle_handle.subscribe();
    handle.spawn(async move {
        // Ready 駆動再接続ループ。supervisor が Ready を立てるまで connect しない。
        loop {
            // (1) 次の Ready を待つ。すでに Ready なら即通過。
            if lifecycle_rx
                .wait_for(|s| matches!(s, BackendLifecycle::Ready))
                .await
                .is_err()
            {
                // watch sender (supervisor) が drop された = アプリ終了。task を畳む。
                return;
            }

            // (2) Ready 到達 → connect。Ready 後の connect は構造的に 1 発成功する想定。
            //     失敗したら Error を出して外側ループへ戻り、次の lifecycle 変化を待つ。
            let mut client = match DataEngineClient::connect(url.clone()).await {
                Ok(c) => {
                    let _ = status_tx.send(BackendStatusUpdate::Connected(true));
                    info!("Backend connection established.");
                    let _ = status_tx.send(BackendStatusUpdate::Running(true));
                    c
                }
                Err(e) => {
                    let err_msg = format!("Failed to connect after Ready: {}", e);
                    error!("{}", err_msg);
                    let _ = status_tx.send(BackendStatusUpdate::Error(err_msg));
                    if lifecycle_rx.changed().await.is_err() {
                        return;
                    }
                    continue;
                }
            };

            // (3) 再接続のたびに initial ListInstruments を必ず 1 回再発火する。
            // Pre-login this falls back to the Replay catalog (plan §3.5 1002);
            // after VenueLogin success we re-fire below to overwrite with the
            // Live venue universe.
            fire_list_instruments(&client, &token, TickersSource::ReplayCatalogFallback, &status_tx);

            // Phase 8 §3.5 subtask 5: dedupe venue_state / execution_mode pushes
            // by tracking the previous raw string we saw from BackendTradingState.
            // dedupe state は inner loop ごとに reset。
            let mut prev_venue: Option<String> = None;
            let mut prev_mode: Option<String> = None;

            // (4) Ready 前 / Restart 中に溜まった古い transport command を破棄する。
            while transport_rx.try_recv().is_ok() {}
            // Phase 8 §3.5 subtask 5: configured_venue の dedupe 用 (prev_venue /
            // prev_mode は上で宣言済み)。inner loop ごとに reset。
            let mut prev_configured_venue: Option<Option<String>> = None;

            // (5) inner main loop: transport drain + GetState polling + lifecycle 監視。
            loop {
                tokio::select! {
                    changed = lifecycle_rx.changed() => {
                        if changed.is_err() {
                            return; // supervisor drop
                        }
                        let state = *lifecycle_rx.borrow();
                        if !matches!(state, BackendLifecycle::Ready) {
                            info!("Backend lifecycle left Ready ({:?}); leaving inner loop.", state);
                            break;
                        }
                    }

                    _ = async {
                        // Drain transport commands before polling state so the UI feels responsive.
                        while let Ok(cmd) = transport_rx.try_recv() {
                            match cmd {
                    TransportCommand::Pause => {
                        let req = tonic::Request::new(PauseReplayRequest {
                            request_id: String::new(),
                            token: token.clone(),
                        });
                        match client.pause_replay(req).await {
                            Ok(r) => info!("PauseReplay ok, state={:?}", r.into_inner().current_state),
                            Err(e) => error!("PauseReplay failed: {}", e),
                        }
                    }
                    TransportCommand::Resume => {
                        let req = tonic::Request::new(ResumeReplayRequest {
                            request_id: String::new(),
                            token: token.clone(),
                        });
                        match client.resume_replay(req).await {
                            Ok(r) => info!("ResumeReplay ok, state={:?}", r.into_inner().current_state),
                            Err(e) => error!("ResumeReplay failed: {}", e),
                        }
                    }
                    TransportCommand::StepForward => {
                        let req = tonic::Request::new(StepReplayRequest {
                            request_id: String::new(),
                            token: token.clone(),
                        });
                        match client.step_replay(req).await {
                            Ok(r) => info!("StepReplay ok, state={:?}", r.into_inner().current_state),
                            Err(e) => error!("StepReplay failed: {}", e),
                        }
                    }
                    TransportCommand::ForceStop => {
                        let req = tonic::Request::new(ForceStopReplayRequest {
                            request_id: String::new(),
                            token: token.clone(),
                        });
                        match client.force_stop_replay(req).await {
                            Ok(r) => info!("ForceStopReplay ok, state={:?}", r.into_inner().current_state),
                            Err(e) => error!("ForceStopReplay failed: {}", e),
                        }
                    }
                    TransportCommand::SetSpeed(mult) => {
                        let req = tonic::Request::new(SetReplaySpeedRequest {
                            request_id: String::new(),
                            multiplier: mult,
                            token: token.clone(),
                        });
                        match client.set_replay_speed(req).await {
                            Ok(r) => info!("SetReplaySpeed {}x ok, state={:?}", mult, r.into_inner().current_state),
                            Err(e) => error!("SetReplaySpeed {}x failed: {}", mult, e),
                        }
                    }
                    TransportCommand::RunStrategy { strategy_file, config, startup_id } => {
                        // Spawn on a separate task so the main loop can process
                        // Pause/Resume/StepForward while StartEngine is running.
                        let mut run_client = client.clone();
                        let run_token = token.clone();
                        let run_catalog = catalog_path.clone();
                        let run_status_tx = status_tx.clone();
                        tokio::spawn(async move {
                            let strategy_file_str = strategy_file.to_string_lossy().to_string();

                            // Step 0: ForceStop to ensure IDLE before LoadReplayData
                            let _ = run_status_tx.send(BackendStatusUpdate::ReplayStartup {
                                startup_id, stage: BackendStartupStage::ResettingReplay,
                            });
                            match run_client.force_stop_replay(tonic::Request::new(ForceStopReplayRequest {
                                request_id: String::new(),
                                token: run_token.clone(),
                            })).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        let msg = format!("ForceStopReplay: {} {}", inner.error_code, inner.error_message);
                                        error!("{}", msg);
                                        let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                            startup_id: Some(startup_id),
                                            error: msg,
                                        });
                                        return;
                                    }
                                    info!("ForceStopReplay ok");
                                }
                                Err(e) => {
                                    let msg = format!("ForceStopReplay gRPC error: {}", e);
                                    error!("{}", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: Some(startup_id),
                                        error: msg,
                                    });
                                    return;
                                }
                            }

                            let granularity_i32 = match parse_replay_granularity(&config.granularity) {
                                Ok(v) => Some(v),
                                Err(msg) => {
                                    error!("RunStrategy: {}, aborting", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: Some(startup_id),
                                        error: msg,
                                    });
                                    return;
                                }
                            };

                            info!(
                                "RunStrategy: step1 LoadReplayData instruments={:?} start={:?} end={:?} granularity={:?} catalog_path={:?}",
                                config.instruments, config.start, config.end, config.granularity, run_catalog
                            );
                            let _ = run_status_tx.send(BackendStatusUpdate::RunStarted);
                            let _ = run_status_tx.send(BackendStatusUpdate::ReplayStartup {
                                startup_id, stage: BackendStartupStage::LoadingData,
                            });

                            // Step 1: LoadReplayData (IDLE → LOADED)
                            let load_req = tonic::Request::new(LoadReplayDataRequest {
                                request_id: String::new(),
                                instrument_ids: config.instruments.clone(),
                                start_date: config.start.clone(),
                                end_date: config.end.clone(),
                                granularity: granularity_i32,
                                token: run_token.clone(),
                                catalog_path: run_catalog.clone(),
                            });

                            match run_client.load_replay_data(load_req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        let msg = format!("LoadReplayData: {} {}", inner.error_code, inner.error_message);
                                        error!("{}", msg);
                                        let _ = run_status_tx.send(BackendStatusUpdate::RunFailed { startup_id: Some(startup_id), error: msg });
                                        return;
                                    }
                                    info!("LoadReplayData ok, state={:?}", inner.current_state);
                                }
                                Err(e) => {
                                    let msg = format!("LoadReplayData gRPC error: {}", e);
                                    error!("{}", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed { startup_id: Some(startup_id), error: msg });
                                    return;
                                }
                            }

                            // Step 2: StartEngine (LOADED → RUNNING → COMPLETED)
                            info!("RunStrategy: step2 StartEngine strategy_file={:?}", strategy_file_str);
                            let _ = run_status_tx.send(BackendStatusUpdate::ReplayStartup {
                                startup_id, stage: BackendStartupStage::StartingStrategy,
                            });
                            let start_req = tonic::Request::new(StartEngineRequest {
                                request_id: String::new(),
                                engine: EngineKind::Nautilus as i32,
                                strategy_id: String::new(),
                                config: Some(EngineStartConfig {
                                    instrument_id: config.instruments.first().cloned().unwrap_or_default(),
                                    instrument_ids: config.instruments.clone(),
                                    start_date: Some(config.start.clone()),
                                    end_date: Some(config.end.clone()),
                                    initial_cash: config.initial_cash.map(|v| v.to_string()),
                                    granularity: granularity_i32,
                                    strategy_file: Some(strategy_file_str),
                                    strategy_init_kwargs: None,
                                    max_qty: None,
                                    max_notional_jpy: None,
                                }),
                                token: run_token.clone(),
                            });
                            match run_client.start_engine(start_req).await {
                                Ok(r) => {
                                    let inner: StartEngineResponse = r.into_inner();
                                    if inner.success {
                                        info!("StartEngine ok, state={:?}", inner.current_state);
                                        let _ = run_status_tx.send(BackendStatusUpdate::ReplayStartup {
                                            startup_id, stage: BackendStartupStage::WaitingForFirstTick,
                                        });
                                        if let (Some(rid), Some(sj)) = (inner.run_id.as_deref(), inner.summary_json.as_deref()) {
                                            let _ = run_status_tx.send(BackendStatusUpdate::RunComplete {
                                                startup_id: Some(startup_id),
                                                run_id: rid.to_owned(),
                                                summary_json: sj.to_owned(),
                                            });
                                        }
                                        // Fetch updated portfolio after run completes.
                                        match run_client.get_portfolio(tonic::Request::new(GetPortfolioRequest {
                                            token: run_token.clone(),
                                        })).await {
                                            Ok(r) => {
                                                let p = r.into_inner();
                                                if p.success {
                                                    let positions = p.positions.into_iter().map(|pos| PortfolioPosition {
                                                        symbol: pos.symbol,
                                                        qty: pos.qty,
                                                        avg_price: pos.avg_price,
                                                        unrealized_pnl: pos.unrealized_pnl,
                                                    }).collect();
                                                    let orders = p.orders.into_iter().map(|o| PortfolioOrder {
                                                        symbol: o.symbol,
                                                        side: o.side,
                                                        qty: o.qty,
                                                        price: o.price,
                                                        status: o.status,
                                                        ts_ms: o.ts_ms,
                                                    }).collect();
                                                    let _ = run_status_tx.send(BackendStatusUpdate::PortfolioLoaded {
                                                        buying_power: p.buying_power,
                                                        cash: p.cash,
                                                        equity: p.equity,
                                                        positions,
                                                        orders,
                                                    });
                                                }
                                            }
                                            Err(e) => warn!("GetPortfolio failed: {}", e),
                                        }
                                    } else {
                                        let msg = format!(
                                            "StartEngine: {} {}",
                                            inner.error_code.as_deref().unwrap_or(""),
                                            inner.error_message.as_deref().unwrap_or(""),
                                        );
                                        error!("{}", msg);
                                        let _ = run_status_tx.send(BackendStatusUpdate::RunFailed { startup_id: Some(startup_id), error: msg });
                                    }
                                }
                                Err(e) => {
                                    let msg = format!("StartEngine gRPC error: {}", e);
                                    error!("{}", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed { startup_id: Some(startup_id), error: msg });
                                }
                            }
                        });
                    }
                    TransportCommand::SetExecutionMode { mode } => {
                        // Spawn so the pump loop is not blocked while the
                        // backend processes the mode switch (sibling commands
                        // like FetchAvailableInstruments / StartEngine follow
                        // the same pattern).
                        let mut sem_client = client.clone();
                        let sem_token = token.clone();
                        tokio::spawn(async move {
                            let req = tonic::Request::new(SetExecutionModeRequest {
                                mode: mode.as_wire_str().to_string(),
                                token: sem_token,
                            });
                            match sem_client.set_execution_mode(req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if inner.success {
                                        info!(
                                            "SetExecutionMode ok, backend execution_mode={}",
                                            inner.execution_mode
                                        );
                                    } else {
                                        error!(
                                            "SetExecutionMode rejected: error_code={:?} target={}",
                                            inner.error_code,
                                            mode.as_wire_str()
                                        );
                                    }
                                }
                                Err(e) => error!("SetExecutionMode failed: {}", e),
                            }
                        });
                    }
                    TransportCommand::FetchAvailableInstruments { end_date } => {
                        let mut fetch_client = client.clone();
                        let fetch_token = token.clone();
                        let fetch_status_tx = status_tx.clone();
                        tokio::spawn(async move {
                            let end_date_str = end_date.format("%Y-%m-%d").to_string();
                            let req = tonic::Request::new(ListAllListedSymbolsRequest {
                                token: fetch_token,
                                end_date: end_date_str.clone(),
                            });
                            match fetch_client.list_all_listed_symbols(req).await {
                                Ok(resp) => {
                                    let inner = resp.into_inner();
                                    if inner.success {
                                        if !inner.resolved_end_date.is_empty()
                                            && inner.resolved_end_date != end_date_str
                                        {
                                            info!(
                                                "ListAllListedSymbols: backend clamped end_date {} -> {} ({} ids)",
                                                end_date_str,
                                                inner.resolved_end_date,
                                                inner.instrument_ids.len()
                                            );
                                        }
                                        let _ = fetch_status_tx.send(BackendStatusUpdate::AvailableInstrumentsLoaded {
                                            end_date,
                                            ids: inner.instrument_ids,
                                        });
                                    } else {
                                        let _ = fetch_status_tx.send(BackendStatusUpdate::AvailableInstrumentsFetchFailed {
                                            end_date,
                                            error: inner.error_message,
                                        });
                                    }
                                }
                                Err(e) => {
                                    let _ = fetch_status_tx.send(BackendStatusUpdate::AvailableInstrumentsFetchFailed {
                                        end_date,
                                        error: e.to_string(),
                                    });
                                }
                            }
                        });
                    }
                    TransportCommand::VenueLogin { venue_id, credentials_source, environment_hint } => {
                        let req = tonic::Request::new(VenueLoginRequest {
                            venue_id: venue_id.clone(),
                            credentials_source,
                            environment_hint,
                            token: token.clone(),
                        });
                        match client.venue_login(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    info!(
                                        "VenueLogin ok: venue_id={} venue_state={} instruments_loaded={}",
                                        venue_id, inner.venue_state, inner.instruments_loaded
                                    );
                                } else {
                                    error!(
                                        "VenueLogin rejected: venue_id={} error_code={}",
                                        venue_id, inner.error_code
                                    );
                                }
                            }
                            Err(e) => error!("VenueLogin failed: venue_id={} err={}", venue_id, e),
                        }
                    }
                    TransportCommand::ListInstruments { source } => {
                        fire_list_instruments(&client, &token, source, &status_tx);
                    }
                    TransportCommand::UnsubscribeMarketData { instrument_id } => {
                        let req = tonic::Request::new(engine::UnsubscribeRequest {
                            instrument_id: instrument_id.clone(),
                            token: token.clone(),
                        });
                        match client.unsubscribe_market_data(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    info!("UnsubscribeMarketData ok: {}", instrument_id);
                                } else {
                                    warn!(
                                        "UnsubscribeMarketData rejected: {} error_code={}",
                                        instrument_id, inner.error_code
                                    );
                                }
                            }
                            Err(e) => error!(
                                "UnsubscribeMarketData failed: {} err={}",
                                instrument_id, e
                            ),
                        }
                    }
                    TransportCommand::SubscribeMarketData { instrument_id } => {
                        let req = tonic::Request::new(SubscribeRequest {
                            instrument_id: instrument_id.clone(),
                            channels: vec!["trades".to_string(), "depth".to_string()],
                            token: token.clone(),
                        });
                        match client.subscribe_market_data(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    info!("SubscribeMarketData ok: {}", instrument_id);
                                } else {
                                    // Plan §3.5 + "案 C": precondition reject is warn-only
                                    // (no ModalLayer yet). LIVE_ADAPTER_NOT_CONFIGURED /
                                    // EXECUTION_MODE_PRECONDITION surfaces here.
                                    warn!(
                                        "SubscribeMarketData rejected: {} error_code={}",
                                        instrument_id, inner.error_code
                                    );
                                }
                            }
                            Err(e) => error!(
                                "SubscribeMarketData failed: {} err={}",
                                instrument_id, e
                            ),
                        }
                    }
                    TransportCommand::VenueLogout => {
                        let req = tonic::Request::new(VenueLogoutRequest {
                            token: token.clone(),
                        });
                        match client.venue_logout(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    info!("VenueLogout ok");
                                } else {
                                    error!("VenueLogout rejected: error_code={}", inner.error_code);
                                }
                            }
                            Err(e) => error!("VenueLogout failed: {}", e),
                        }
                    }
                    TransportCommand::PlaceOrder {
                        venue,
                        instrument_id,
                        side,
                        qty,
                        price,
                        order_type,
                        time_in_force,
                        second_secret,
                    } => {
                        let req = tonic::Request::new(engine::PlaceOrderReq {
                            token: token.clone(),
                            venue: venue.clone(),
                            instrument_id: instrument_id.clone(),
                            side: side.clone(),
                            qty,
                            price,
                            order_type: order_type.clone(),
                            time_in_force: time_in_force.clone(),
                            // Plaintext leaves Rust only here, copied straight into the
                            // gRPC request; `second_secret` (RedactedSecret) is dropped
                            // with the command at the end of this arm (Phase 9 §1.3).
                            second_secret: second_secret.as_ref().map(|s| s.expose().to_string()),
                        });
                        match client.place_order(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    if let Some(ev) = inner.order_event {
                                        info!(
                                            "PlaceOrder ok: {} {} {} qty={} status={} client_order_id={}",
                                            venue, side, instrument_id, qty, ev.status, ev.client_order_id
                                        );
                                        // Merge the command's static fields (symbol/side/qty/
                                        // price — absent from OrderEvent) with the response ids
                                        // + status, then upsert into LiveOrders for the panel.
                                        let _ = status_tx.send(BackendStatusUpdate::OrderSeeded {
                                            client_order_id: ev.client_order_id,
                                            venue_order_id: ev.venue_order_id,
                                            symbol: instrument_id.clone(),
                                            side: side.clone(),
                                            qty,
                                            price,
                                            status: ev.status,
                                            filled_qty: ev.filled_qty,
                                            avg_price: ev.avg_price,
                                            ts_ms: ev.ts_ms,
                                        });
                                    } else {
                                        warn!("PlaceOrder ok but no order_event returned: {}", instrument_id);
                                    }
                                } else {
                                    // Replay → EXECUTION_MODE_PRECONDITION, runner not up →
                                    // VENUE_LOGIN_REQUIRED (structured errors, not gRPC abort).
                                    warn!(
                                        "PlaceOrder rejected: {} error_code={}",
                                        instrument_id, inner.error_code
                                    );
                                    let _ = status_tx.send(BackendStatusUpdate::OrderRejected {
                                        action: "発注".to_string(),
                                        error_code: inner.error_code,
                                    });
                                }
                            }
                            Err(e) => error!("PlaceOrder failed: {} err={}", instrument_id, e),
                        }
                    }
                    TransportCommand::CancelOrder {
                        venue,
                        order_id,
                        second_secret,
                    } => {
                        let req = tonic::Request::new(engine::CancelOrderReq {
                            token: token.clone(),
                            venue: venue.clone(),
                            order_id: order_id.clone(),
                            second_secret: second_secret.as_ref().map(|s| s.expose().to_string()),
                        });
                        match client.cancel_order(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    if let Some(ev) = inner.order_event {
                                        info!(
                                            "CancelOrder ok: order_id={} status={}",
                                            order_id, ev.status
                                        );
                                        // Cancel response carries no symbol/side/qty/price;
                                        // OrderStatusUpdated merges status into the existing record.
                                        let _ = status_tx.send(
                                            BackendStatusUpdate::OrderStatusUpdated {
                                                client_order_id: ev.client_order_id,
                                                venue_order_id: ev.venue_order_id,
                                                status: ev.status,
                                                filled_qty: ev.filled_qty,
                                                avg_price: ev.avg_price,
                                                ts_ms: ev.ts_ms,
                                            },
                                        );
                                    }
                                } else {
                                    warn!(
                                        "CancelOrder rejected: order_id={} error_code={}",
                                        order_id, inner.error_code
                                    );
                                    let _ = status_tx.send(BackendStatusUpdate::OrderRejected {
                                        action: "取消".to_string(),
                                        error_code: inner.error_code,
                                    });
                                }
                            }
                            Err(e) => error!("CancelOrder failed: order_id={} err={}", order_id, e),
                        }
                    }
                    TransportCommand::ModifyOrder {
                        venue,
                        client_order_id,
                        new_qty,
                        new_price,
                        second_secret,
                    } => {
                        let req = tonic::Request::new(engine::ModifyOrderReq {
                            token: token.clone(),
                            venue: venue.clone(),
                            order_id: client_order_id.clone(),
                            new_price,
                            new_qty,
                            second_secret: second_secret.as_ref().map(|s| s.expose().to_string()),
                        });
                        match client.modify_order(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    if let Some(ev) = inner.order_event {
                                        info!(
                                            "ModifyOrder ok: client_order_id={} status={}",
                                            client_order_id, ev.status
                                        );
                                        // OrderEvent carries ids + status + fills but no
                                        // qty/price, so merge the command's new_qty/new_price.
                                        let _ = status_tx.send(BackendStatusUpdate::OrderModified {
                                            client_order_id: ev.client_order_id,
                                            venue_order_id: ev.venue_order_id,
                                            new_qty,
                                            new_price,
                                            status: ev.status,
                                            filled_qty: ev.filled_qty,
                                            avg_price: ev.avg_price,
                                            ts_ms: ev.ts_ms,
                                        });
                                    } else {
                                        warn!(
                                            "ModifyOrder ok but no order_event returned: {}",
                                            client_order_id
                                        );
                                    }
                                } else {
                                    warn!(
                                        "ModifyOrder rejected: client_order_id={} error_code={}",
                                        client_order_id, inner.error_code
                                    );
                                    let _ = status_tx.send(BackendStatusUpdate::OrderRejected {
                                        action: "訂正".to_string(),
                                        error_code: inner.error_code,
                                    });
                                }
                            }
                            Err(e) => error!(
                                "ModifyOrder failed: client_order_id={} err={}",
                                client_order_id, e
                            ),
                        }
                    }
                    TransportCommand::SubmitSecret { request_id, secret } => {
                        let req = tonic::Request::new(engine::SubmitSecretReq {
                            token: token.clone(),
                            request_id: request_id.clone(),
                            // Plaintext is copied into the request and the command (with its
                            // RedactedSecret) is dropped at the end of this arm.
                            secret: secret.expose().to_string(),
                        });
                        match client.submit_secret(req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if inner.success {
                                    info!("SubmitSecret ok: request_id={}", request_id);
                                } else {
                                    warn!(
                                        "SubmitSecret rejected: request_id={} error_code={}",
                                        request_id, inner.error_code
                                    );
                                    let _ = status_tx.send(BackendStatusUpdate::OrderRejected {
                                        action: "第二暗証番号".to_string(),
                                        error_code: inner.error_code,
                                    });
                                }
                            }
                            Err(e) => error!("SubmitSecret failed: request_id={} err={}", request_id, e),
                        }
                    }
                }
            }

                        let request = tonic::Request::new(GetStateRequest {
                            token: token.clone(),
                        });

                        match tokio::time::timeout(tokio::time::Duration::from_secs(5), client.get_state(request)).await {
                Ok(Ok(response)) => {
                    let json_data = response.into_inner().json_data;
                    match serde_json::from_str::<backcast::trading::BackendTradingState>(&json_data) {
                        Ok(state) => {
                            // Phase 8 §3.5 subtask 5: detect venue_state / execution_mode
                            // transitions and push typed updates. venue_id / instruments_loaded
                            // are now sourced from backend BackendTradingState.
                            if state.venue_state != prev_venue {
                                if let Some(ref s) = state.venue_state {
                                    match parse_venue_state(s) {
                                        Some(vs) => {
                                            let _ = status_tx.send(BackendStatusUpdate::VenueChanged {
                                                state: vs,
                                                venue_id: state.venue_id.clone(),
                                                instruments_loaded: state.instruments_loaded.unwrap_or(0),
                                            });
                                            // D15: venue-transition fire_list_instruments removed.
                                            // Live universe is now auto-fetched by
                                            // auto_fetch_live_universe_on_connect_system (§4.6.1).
                                        }
                                        None => warn!("unknown venue_state from backend: {:?}", s),
                                    }
                                }
                                prev_venue = state.venue_state.clone();
                            }
                            if state.execution_mode != prev_mode {
                                if let Some(ref m) = state.execution_mode {
                                    match parse_execution_mode(m) {
                                        Some(em) => {
                                            let _ = status_tx.send(BackendStatusUpdate::ExecutionModeChanged {
                                                mode: em,
                                            });
                                        }
                                        None => warn!("unknown execution_mode from backend: {:?}", m),
                                    }
                                }
                                prev_mode = state.execution_mode.clone();
                            }
                            if prev_configured_venue.as_ref() != Some(&state.configured_venue) {
                                let _ = status_tx.send(BackendStatusUpdate::ConfiguredVenueDiscovered {
                                    venue_id: state.configured_venue.clone(),
                                });
                                prev_configured_venue = Some(state.configured_venue.clone());
                            }
                            // Phase 8 §3.5: push last_prices map as a typed
                            // status update. Overwrite semantics — Replay 切替
                            // 時は backend が空 map を返すので sidebar が clear される。
                            let _ = status_tx.send(BackendStatusUpdate::LastPricesUpdated {
                                prices: state.last_prices.clone(),
                            });
                            let _ = tx.send(state);
                            let _ = status_tx.send(BackendStatusUpdate::Connected(true));
                        }
                        Err(e) => {
                            let err_msg = format!("JSON parse error: {}. Data: {}", e, json_data);
                            error!("{}", err_msg);
                            let _ = status_tx.send(BackendStatusUpdate::Error(err_msg));
                        }
                    }
                }
                        Ok(Err(e)) => {
                            let err_msg = format!("gRPC error: {}", e);
                            error!("{}", err_msg);
                            let _ = status_tx.send(BackendStatusUpdate::Error(err_msg));
                        }
                        Err(_) => {
                            // Backend busy (e.g. during LoadReplayData / engine_runner.run).
                            // Not a connection failure — skip reconnect to avoid noise.
                            warn!("GetState timed out (backend busy), will retry next poll");
                        }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
                    } => {}
                }
            }
        }
    });

    // Backend event subscriber: own client + own Ready-driven reconnect loop
    // (cannot share the status task's client, which is busy in its select! loop).
    let ev_url = settings.backend_url.clone();
    let ev_token = settings.token.clone();
    let mut ev_lifecycle_rx = lifecycle_handle.subscribe();
    let ev_handle = tokio_handle.0.clone();
    ev_handle.spawn(async move {
        loop {
            if ev_lifecycle_rx
                .wait_for(|s| matches!(s, BackendLifecycle::Ready))
                .await
                .is_err()
            {
                return; // supervisor dropped = app exit
            }
            let mut client = match DataEngineClient::connect(ev_url.clone()).await {
                Ok(c) => c,
                Err(e) => {
                    error!("[backend-events] connect failed after Ready: {}", e);
                    if !events_reconnect_backoff(&mut ev_lifecycle_rx).await {
                        return; // supervisor dropped = app exit
                    }
                    continue;
                }
            };
            let req = tonic::Request::new(SubscribeBackendEventsReq {
                token: ev_token.clone(),
            });
            let mut stream = match client.subscribe_backend_events(req).await {
                Ok(resp) => resp.into_inner(),
                Err(e) => {
                    error!("[backend-events] subscribe failed: {}", e);
                    if !events_reconnect_backoff(&mut ev_lifecycle_rx).await {
                        return; // supervisor dropped = app exit
                    }
                    continue;
                }
            };
            info!("[backend-events] stream established.");
            loop {
                match stream.message().await {
                    Ok(Some(ev)) => {
                        let Some(payload) = ev.payload else {
                            warn!("[backend-events] event with empty payload; skipping");
                            continue;
                        };
                        let mapped = match payload {
                            engine::backend_event::Payload::SecretRequired(p) => {
                                backcast::trading::BackendEvent::SecretRequired {
                                    request_id: p.request_id,
                                    venue: p.venue,
                                    kind: p.kind,
                                    purpose: p.purpose,
                                }
                            }
                            engine::backend_event::Payload::OrderEvent(p) => {
                                backcast::trading::BackendEvent::OrderEvent {
                                    order_id: p.order_id,
                                    venue_order_id: p.venue_order_id,
                                    client_order_id: p.client_order_id,
                                    status: p.status,
                                    filled_qty: p.filled_qty,
                                    avg_price: p.avg_price,
                                    ts_ms: p.ts_ms,
                                }
                            }
                            engine::backend_event::Payload::AccountEvent(p) => {
                                backcast::trading::BackendEvent::AccountEvent {
                                    cash: p.cash,
                                    buying_power: p.buying_power,
                                    positions: p
                                        .positions
                                        .into_iter()
                                        .map(|pos| backcast::trading::AccountPosition {
                                            symbol: pos.symbol,
                                            qty: pos.qty,
                                            avg_price: pos.avg_price,
                                            unrealized_pnl: pos.unrealized_pnl,
                                        })
                                        .collect(),
                                    ts_ms: p.ts_ms,
                                }
                            }
                            engine::backend_event::Payload::VenueLogoutDetected(p) => {
                                backcast::trading::BackendEvent::VenueLogoutDetected {
                                    venue: p.venue,
                                }
                            }
                        };
                        let _ = event_tx.send(mapped);
                    }
                    Ok(None) => {
                        info!("[backend-events] server closed stream; reconnecting.");
                        break;
                    }
                    Err(e) => {
                        error!("[backend-events] stream error: {}; reconnecting.", e);
                        break;
                    }
                }
            }
            // The stream ended (server closed it or errored) while we may still
            // be Ready; back off, then loop to wait_for(Ready) and reconnect.
            if !events_reconnect_backoff(&mut ev_lifecycle_rx).await {
                return; // supervisor dropped = app exit
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        BackendLifecycle, ReplayGranularity, ReplayStartupProgress, parse_replay_granularity,
        should_send_graceful_shutdown,
    };
    use backcast::backend_sync::{
        apply_account_event, apply_available_failed, apply_available_loaded, apply_status_update,
    };
    use backcast::replay::ReplayStartupPhase;
    use backcast::trading::{
        AccountPosition, AvailableInstruments, BackendStartupStage, BackendStatus,
        BackendStatusUpdate, ExecutionModeRes, LastPrices, LastRunResult, LiveOrders,
        OrderFeedback, PortfolioState, RunState, Ticker, Tickers, VenueStatusRes,
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
        // position fields map through faithfully.
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

    fn apply(update: BackendStatusUpdate, progress: &mut ReplayStartupProgress) -> LastRunResult {
        let mut status = BackendStatus::default();
        let mut last_run = LastRunResult::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut tickers = Tickers::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
        );
        last_run
    }

    /// Variant of `apply` that seeds an initial `OrderFeedback` and returns it,
    /// for verifying the order-notice set/clear behavior (Round 2 M-A/M-B).
    fn apply_feedback(update: BackendStatusUpdate, initial: Option<&str>) -> OrderFeedback {
        let mut status = BackendStatus::default();
        let mut last_run = LastRunResult::default();
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
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            &mut progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
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
        let mut last_run = LastRunResult::default();
        // Seed a "Live" portfolio snapshot (as apply_account_event would leave it).
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
        // Currently in LiveManual; the backend reports a switch to Replay.
        let mut exec_mode = ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        };
        let mut tickers = Tickers::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        apply_status_update(
            BackendStatusUpdate::ExecutionModeChanged {
                mode: ExecutionMode::Replay,
            },
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            &mut progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
        );
        assert!(
            !portfolio.loaded,
            "stale Live account snapshot must not bleed into the Replay view"
        );
        assert_eq!(portfolio.cash, 0.0);
        assert_eq!(portfolio.equity, 0.0);
        assert_eq!(portfolio.buying_power, 0.0);
    }

    /// Variant of `apply` that returns the `Tickers` resource after the update,
    /// for verifying `InstrumentsListed` overwrite semantics.
    fn apply_with_tickers(
        update: BackendStatusUpdate,
        progress: &mut ReplayStartupProgress,
        tickers: &mut Tickers,
    ) {
        let mut status = BackendStatus::default();
        let mut last_run = LastRunResult::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            progress,
            &mut venue_status,
            &mut exec_mode,
            tickers,
            &mut last_prices,
            &mut live_orders,
            &mut order_feedback,
        );
    }

    #[test]
    fn graceful_shutdown_only_for_live_states() {
        // Backend が動いていない / 終了済みの状態では Shutdown を送らない
        // (supervisor の cmd_rx が drop 済みで ack が返らず 2.5s timeout を焼くだけ)。
        assert!(!should_send_graceful_shutdown(BackendLifecycle::Disabled));
        assert!(!should_send_graceful_shutdown(BackendLifecycle::Stopped));
        assert!(!should_send_graceful_shutdown(BackendLifecycle::Crashed));
        assert!(!should_send_graceful_shutdown(
            BackendLifecycle::StartupFailed("BACKEND_NOT_REACHABLE")
        ));

        // 起動中 / 稼働中 / shutdown 進行中は ack を待つ価値があるので送る。
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
        let last_run = apply(
            BackendStatusUpdate::RunFailed {
                startup_id: Some(7),
                error: "boom".into(),
            },
            &mut progress,
        );
        assert_eq!(progress.error.as_deref(), Some("boom"));
        assert_eq!(
            last_run.state,
            RunState::Failed {
                error: "boom".into()
            }
        );
    }

    #[test]
    fn apply_run_failed_with_none_startup_id_only_updates_last_run() {
        let mut progress = fresh_progress(7, true);
        let last_run = apply(
            BackendStatusUpdate::RunFailed {
                startup_id: None,
                error: "boom".into(),
            },
            &mut progress,
        );
        assert!(progress.error.is_none());
        assert_eq!(
            last_run.state,
            RunState::Failed {
                error: "boom".into()
            }
        );
    }

    #[test]
    fn apply_run_failed_with_mismatched_startup_id_ignored() {
        let mut progress = fresh_progress(7, true);
        let last_run = apply(
            BackendStatusUpdate::RunFailed {
                startup_id: Some(9),
                error: "boom".into(),
            },
            &mut progress,
        );
        assert!(progress.error.is_none());
        assert_eq!(
            last_run.state,
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
        let last_run = apply(
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
        assert!(matches!(last_run.state, RunState::Completed { .. }));
    }

    /// #7b strict: a `RunComplete` whose `startup_id == None` (legacy / unrelated)
    /// must not close a freshly-opened progress window. Pairs with the matching
    /// `apply_run_complete_with_stale_startup_id_keeps_progress` test for
    /// `Some(other)` and adds the `None` case the plan calls out explicitly.
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

    #[test]
    fn parse_replay_granularity_daily() {
        assert_eq!(
            parse_replay_granularity("Daily").unwrap(),
            ReplayGranularity::Daily as i32
        );
    }

    #[test]
    fn parse_replay_granularity_minute() {
        assert_eq!(
            parse_replay_granularity("Minute").unwrap(),
            ReplayGranularity::Minute as i32
        );
    }

    #[test]
    fn parse_replay_granularity_unknown_returns_err() {
        let err = parse_replay_granularity("Hourly").unwrap_err();
        assert!(err.contains("Hourly"));
    }

    #[test]
    fn parse_replay_granularity_empty_returns_err() {
        assert!(parse_replay_granularity("").is_err());
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

    /// Variant of `apply` that lets the caller seed and observe a `LastPrices`
    /// resource across the `apply_status_update` call (mirrors the
    /// `apply_with_tickers` helper above).
    fn apply_with_last_prices(
        update: BackendStatusUpdate,
        progress: &mut ReplayStartupProgress,
        last_prices: &mut LastPrices,
    ) {
        let mut status = BackendStatus::default();
        let mut last_run = LastRunResult::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut tickers = Tickers::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            last_prices,
            &mut live_orders,
            &mut order_feedback,
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
