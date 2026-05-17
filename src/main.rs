use backcast::camera::{pancam_suppression_over_editor_system, setup_camera};
use backcast::grid::GridPlugin;
use backcast::trading::{
    AvailableInstruments, BackendChannel, BackendStatus, LastRunResult, PortfolioOrder,
    PortfolioPosition, PortfolioState, ReplaySpeed, RunState, TradingData, TradingSettings,
    TransportCommand, TransportCommandSender, backend_update_system, engine, parse_summary_json,
    price_simulation_system,
};
use backcast::replay::{ReplayStartupPhase, ReplayStartupProgress};
use backcast::ui::UiPlugin;
use bevy::prelude::*;
use bevy_pancam::{PanCamPlugin, PanCamSystemSet};
use chrono::NaiveDate;
use engine::data_engine_client::DataEngineClient;
use engine::{
    EngineKind, EngineStartConfig, ForceStopReplayRequest, GetPortfolioRequest, GetStateRequest,
    ListAllListedSymbolsRequest, LoadReplayDataRequest, PauseReplayRequest, ReplayGranularity,
    ResumeReplayRequest, SetReplaySpeedRequest, StartEngineRequest, StartEngineResponse,
    StepReplayRequest,
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
        .add_plugins(PanCamPlugin)
        .add_plugins(UiPlugin)
        .add_plugins(GridPlugin)
        .insert_resource(TradingData::default())
        .insert_resource(TradingSettings::default())
        .insert_resource(BackendStatus::default())
        .insert_resource(LastRunResult::default())
        .init_resource::<ReplayStartupProgress>()
        .insert_resource(AvailableInstruments::default())
        .insert_resource(PortfolioState::default())
        .insert_resource(ReplaySpeed::default())
        .insert_resource(tokio_handle)
        .add_systems(Startup, (setup_camera, setup_backend_connection))
        .add_systems(
            Update,
            (
                price_simulation_system,
                backend_update_system,
                status_update_system,
                // PanCam の do_camera_zoom より前に走らせ、enabled フラグを先に確定させる。
                pancam_suppression_over_editor_system.before(PanCamSystemSet),
            ),
        )
        .run();
}

#[derive(Resource)]
struct StatusUpdateChannel {
    rx: mpsc::UnboundedReceiver<BackendStatusUpdate>,
}

enum BackendStartupStage {
    ResettingReplay,
    LoadingData,
    StartingStrategy,
    WaitingForFirstTick,
}

enum BackendStatusUpdate {
    Connected(bool),
    Running(bool),
    Error(String),
    RunStarted,
    ReplayStartup {
        startup_id: u64,
        stage: BackendStartupStage,
    },
    RunComplete {
        startup_id: Option<u64>,
        run_id: String,
        summary_json: String,
    },
    RunFailed {
        startup_id: Option<u64>,
        error: String,
    },
    PortfolioLoaded {
        buying_power: f64,
        cash: f64,
        equity: f64,
        positions: Vec<PortfolioPosition>,
        orders: Vec<PortfolioOrder>,
    },
    AvailableInstrumentsLoaded {
        end_date: NaiveDate,
        ids: Vec<String>,
    },
    AvailableInstrumentsFetchFailed {
        end_date: NaiveDate,
        error: String,
    },
}

fn status_update_system(
    mut status: ResMut<BackendStatus>,
    mut channel: ResMut<StatusUpdateChannel>,
    mut last_run: ResMut<LastRunResult>,
    mut portfolio: ResMut<PortfolioState>,
    mut available: ResMut<AvailableInstruments>,
    mut progress: ResMut<ReplayStartupProgress>,
) {
    while let Ok(update) = channel.rx.try_recv() {
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            &mut progress,
        );
    }
}

fn apply_status_update(
    update: BackendStatusUpdate,
    status: &mut BackendStatus,
    last_run: &mut LastRunResult,
    portfolio: &mut PortfolioState,
    available: &mut AvailableInstruments,
    progress: &mut ReplayStartupProgress,
) {
    match update {
        BackendStatusUpdate::Connected(c) => status.connected = c,
        BackendStatusUpdate::Running(r) => status.running = r,
        BackendStatusUpdate::Error(e) => {
            status.last_error = Some(e);
            status.connected = false;
        }
        BackendStatusUpdate::RunStarted => {
            last_run.state = RunState::Running;
        }
        BackendStatusUpdate::ReplayStartup { startup_id, stage } => {
            if progress.visible && progress.startup_id == startup_id {
                progress.phase = match stage {
                    BackendStartupStage::ResettingReplay => ReplayStartupPhase::ResettingReplay,
                    BackendStartupStage::LoadingData => ReplayStartupPhase::LoadingData,
                    BackendStartupStage::StartingStrategy => ReplayStartupPhase::StartingStrategy,
                    BackendStartupStage::WaitingForFirstTick => {
                        ReplayStartupPhase::WaitingForFirstTick
                    }
                };
                if matches!(stage, BackendStartupStage::WaitingForFirstTick) {
                    progress.start_engine_accepted = true;
                }
            }
        }
        BackendStatusUpdate::RunComplete {
            startup_id,
            run_id,
            summary_json,
        } => {
            info!("RunComplete: run_id={} summary={}", run_id, summary_json);
            last_run.parsed_summary = parse_summary_json(&summary_json);
            last_run.run_id = Some(run_id);
            last_run.summary_json = Some(summary_json);
            last_run.state = RunState::Completed;

            if let Some(sid) = startup_id
                && progress.visible
                && progress.startup_id == sid
            {
                progress.visible = false;
                progress.phase = ReplayStartupPhase::Idle;
                progress.detail = None;
                progress.baseline_timestamp_ms = None;
                progress.started_at_elapsed = None;
                progress.start_engine_accepted = false;
            }
        }
        BackendStatusUpdate::RunFailed { startup_id, error } => {
            if let Some(sid) = startup_id
                && progress.visible
                && progress.startup_id == sid
            {
                progress.error = Some(error.clone());
            }
            last_run.state = RunState::Failed { error };
        }
        BackendStatusUpdate::PortfolioLoaded {
            buying_power,
            cash,
            equity,
            positions,
            orders,
        } => {
            portfolio.buying_power = buying_power;
            portfolio.cash = cash;
            portfolio.equity = equity;
            portfolio.positions = positions;
            portfolio.orders = orders;
            portfolio.loaded = true;
        }
        BackendStatusUpdate::AvailableInstrumentsLoaded { end_date, ids } => {
            apply_available_loaded(available, end_date, ids);
        }
        BackendStatusUpdate::AvailableInstrumentsFetchFailed { end_date, error } => {
            apply_available_failed(available, end_date, error);
        }
    }
}

fn apply_available_loaded(
    available: &mut AvailableInstruments,
    end_date: NaiveDate,
    ids: Vec<String>,
) {
    available.by_end_date.insert(end_date, ids);
    available.in_flight.remove(&end_date);
}

fn apply_available_failed(
    available: &mut AvailableInstruments,
    end_date: NaiveDate,
    error: String,
) {
    available.last_error = Some((end_date, error));
    available.in_flight.remove(&end_date);
}

fn setup_backend_connection(
    mut commands: Commands,
    settings: Res<TradingSettings>,
    tokio_handle: Res<TokioHandle>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendChannel { rx });

    let (status_tx, status_rx) = mpsc::unbounded_channel();
    commands.insert_resource(StatusUpdateChannel { rx: status_rx });

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
    handle.spawn(async move {
        let mut client = loop {
            match DataEngineClient::connect(url.clone()).await {
                Ok(c) => {
                    let _ = status_tx.send(BackendStatusUpdate::Connected(true));
                    break c;
                }
                Err(e) => {
                    let err_msg = format!("Failed to connect: {}", e);
                    error!("{}", err_msg);
                    let _ = status_tx.send(BackendStatusUpdate::Error(err_msg));
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        };

        // Backend manages its own lifecycle; no explicit Start call needed.
        info!("Backend connection established.");
        let _ = status_tx.send(BackendStatusUpdate::Running(true));

        loop {
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
                    error!("{}. Attempting to reconnect...", err_msg);
                    let _ = status_tx.send(BackendStatusUpdate::Error(err_msg));
                    if let Ok(c) = DataEngineClient::connect(url.clone()).await {
                        client = c;
                        let _ = status_tx.send(BackendStatusUpdate::Connected(true));
                    }
                }
                Err(_) => {
                    // Backend busy (e.g. during LoadReplayData / engine_runner.run).
                    // Not a connection failure — skip reconnect to avoid noise.
                    warn!("GetState timed out (backend busy), will retry next poll");
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        BackendStartupStage, BackendStatusUpdate, ReplayGranularity, ReplayStartupPhase,
        ReplayStartupProgress, apply_available_failed, apply_available_loaded,
        apply_status_update, parse_replay_granularity,
    };
    use backcast::trading::{
        AvailableInstruments, BackendStatus, LastRunResult, PortfolioState, RunState,
    };
    use chrono::NaiveDate;

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
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            progress,
        );
        last_run
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
        apply(
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
        assert_eq!(parse_replay_granularity("Daily").unwrap(), ReplayGranularity::Daily as i32);
    }

    #[test]
    fn parse_replay_granularity_minute() {
        assert_eq!(parse_replay_granularity("Minute").unwrap(), ReplayGranularity::Minute as i32);
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
}
