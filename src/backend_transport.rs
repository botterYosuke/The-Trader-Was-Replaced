use std::pin::Pin;
use std::future::Future;
use tokio::sync::mpsc;
use bevy::log::{error, info, warn};
use pyo3::prelude::{Py, PyAny};

use crate::backend_supervisor::BackendLifecycle;
use crate::backend_sync::lifecycle_status_update;
use crate::trading::{
    AccountPosition, BackendEvent, BackendStartupStage, BackendStatusUpdate, BackendTradingState,
    LiveOrder, PortfolioOrder, PortfolioPosition, Ticker, TickersSource, TransportCommand,
    VenueState, ExecutionMode, default_live_auto_safety_limits, get_orders_notice,
    reconcile_ids_for_seed, tickers_source_to_wire, engine,
};
use engine::data_engine_client::DataEngineClient;
use engine::{
    EngineKind, EngineStartConfig, ForceAccountSnapshotRequest, ForceStopReplayRequest,
    GetPortfolioRequest, GetStateRequest,
    ListAllListedSymbolsRequest, ListInstrumentsRequest, LoadReplayDataRequest,
    PauseLiveStrategyReq, PauseReplayRequest, RegisterLiveStrategyReq, ReplayGranularity,
    ResumeLiveStrategyReq, ResumeReplayRequest, SafetyLimits, SetExecutionModeRequest,
    SetReplaySpeedRequest, StartEngineRequest, StartEngineResponse, StartLiveStrategyReq,
    StepReplayRequest, StopLiveStrategyReq, SubscribeBackendEventsReq, SubscribeRequest,
    VenueLoginRequest, VenueLogoutRequest,
};

use crate::backend_sync::{build_register_reject_message, build_start_reject_message};

/// Abstraction over the backend communication channel.
///
/// An implementor owns the transport loop: it reads `TransportCommand`s from
/// `transport_rx`, forwards them to the backend, and pushes state/status/events
/// back on the three sender halves.  The lifecycle receiver drives connect /
/// reconnect — the transport must honour `BackendLifecycle::Ready` as the gate.
///
/// The `Pin<Box<dyn Future>>` return keeps the trait object-safe so Phase 2 can
/// swap `GrpcTransport` for `InProcTransport` via `Box<dyn BackendTransport>`.
pub trait BackendTransport: Send + 'static {
    fn run(
        self: Box<Self>,
        transport_rx: mpsc::UnboundedReceiver<TransportCommand>,
        state_tx: mpsc::UnboundedSender<BackendTradingState>,
        status_tx: mpsc::UnboundedSender<BackendStatusUpdate>,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
        lifecycle_rx: tokio::sync::watch::Receiver<BackendLifecycle>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
}

// ---------------------------------------------------------------------------
// GrpcTransport — the existing TCP+protobuf implementation
// ---------------------------------------------------------------------------

pub struct GrpcTransport {
    pub url: String,
    pub token: String,
    pub poll_interval_ms: u64,
    pub catalog_path: Option<String>,
}

impl BackendTransport for GrpcTransport {
    fn run(
        self: Box<Self>,
        mut transport_rx: mpsc::UnboundedReceiver<TransportCommand>,
        state_tx: mpsc::UnboundedSender<BackendTradingState>,
        status_tx: mpsc::UnboundedSender<BackendStatusUpdate>,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
        mut lifecycle_rx: tokio::sync::watch::Receiver<BackendLifecycle>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        let url = self.url;
        let token = self.token;
        let interval = self.poll_interval_ms;
        let catalog_path = self.catalog_path;

        // Single-flight serialization for SetExecutionMode (issue #3). Mode-switch
        // RPCs must reach the backend in click order, otherwise two switches within
        // one poll interval can leave the backend on the *earlier* target. We spawn
        // each switch (so the pump loop never head-of-line blocks on it) but gate
        // the actual RPC behind `mode_gate` so only one is in flight at a time, and
        // tag each click with a monotonic `mode_seq` so a switch superseded before
        // it acquires the gate is dropped — structural last-click-wins.
        let mode_seq = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mode_gate = std::sync::Arc::new(tokio::sync::Mutex::new(()));

        Box::pin(async move {
            // Backend event subscriber runs on its own task (cannot share the command
            // task's client, which is busy in its select! loop).
            let ev_url = url.clone();
            let ev_token = token.clone();
            let ev_event_tx = event_tx.clone();
            let mut ev_lifecycle_rx = lifecycle_rx.clone();
            tokio::spawn(async move {
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
                                return;
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
                                return;
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
                                let _ = ev_event_tx.send(map_backend_event_payload(payload));
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
                    if !events_reconnect_backoff(&mut ev_lifecycle_rx).await {
                        return;
                    }
                }
            });

            // Ready-driven reconnect loop. We do not connect until the supervisor
            // signals Ready.
            loop {
                // (1) Wait for Ready; surface terminal StartupFailed states.
                loop {
                    let s = *lifecycle_rx.borrow();
                    if matches!(s, BackendLifecycle::Ready) {
                        break;
                    }
                    if let Some(update) = lifecycle_status_update(s) {
                        let _ = status_tx.send(update);
                    }
                    if lifecycle_rx.changed().await.is_err() {
                        return;
                    }
                }

                // (2) Connect.
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

                // (3) Initial ListInstruments on every reconnect.
                fire_list_instruments(
                    &client,
                    &token,
                    TickersSource::ReplayCatalogFallback,
                    &status_tx,
                );

                let mut prev_venue: Option<String> = None;
                let mut prev_mode: Option<String> = None;

                // (4) Selective flush of stale commands accumulated during restart.
                let mut drained: Vec<TransportCommand> = Vec::new();
                while let Ok(cmd) = transport_rx.try_recv() {
                    drained.push(cmd);
                }
                let mut preserved_cmds = flush_stale_transport_commands(drained);
                let mut prev_configured_venue: Option<Option<String>> = None;

                // (5) Inner loop: drain commands + poll GetState + watch lifecycle.
                loop {
                    tokio::select! {
                        changed = lifecycle_rx.changed() => {
                            if changed.is_err() {
                                return;
                            }
                            let state = *lifecycle_rx.borrow();
                            if !matches!(state, BackendLifecycle::Ready) {
                                info!("Backend lifecycle left Ready ({:?}); leaving inner loop.", state);
                                break;
                            }
                        }

                        _ = async {
                            while let Some(cmd) = preserved_cmds.pop_front().or_else(|| transport_rx.try_recv().ok()) {
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
                    TransportCommand::LoadAndStep { config, startup_id } => {
                        let mut run_client = client.clone();
                        let run_token = token.clone();
                        let run_catalog = catalog_path.clone();
                        let run_status_tx = status_tx.clone();
                        tokio::spawn(async move {
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
                                        let msg = format!("LoadAndStep ForceStop: {} {}", inner.error_code, inner.error_message);
                                        error!("{}", msg);
                                        let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                            startup_id: Some(startup_id), error: msg,
                                        });
                                        return;
                                    }
                                }
                                Err(e) => {
                                    let msg = format!("LoadAndStep ForceStop gRPC error: {}", e);
                                    error!("{}", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: Some(startup_id), error: msg,
                                    });
                                    return;
                                }
                            }

                            let granularity_i32 = match parse_replay_granularity(&config.granularity) {
                                Ok(v) => Some(v),
                                Err(msg) => {
                                    error!("LoadAndStep: {}, aborting", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: Some(startup_id), error: msg,
                                    });
                                    return;
                                }
                            };

                            let _ = run_status_tx.send(BackendStatusUpdate::ReplayStartup {
                                startup_id, stage: BackendStartupStage::LoadingData,
                            });
                            match run_client.load_replay_data(tonic::Request::new(LoadReplayDataRequest {
                                request_id: String::new(),
                                instrument_ids: config.instruments.clone(),
                                start_date: config.start.clone(),
                                end_date: config.end.clone(),
                                granularity: granularity_i32,
                                token: run_token.clone(),
                                catalog_path: run_catalog.clone(),
                            })).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        let msg = format!("LoadAndStep LoadReplayData: {} {}", inner.error_code, inner.error_message);
                                        error!("{}", msg);
                                        let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                            startup_id: Some(startup_id), error: msg,
                                        });
                                        return;
                                    }
                                    info!("LoadAndStep: LoadReplayData ok");
                                }
                                Err(e) => {
                                    let msg = format!("LoadAndStep LoadReplayData gRPC error: {}", e);
                                    error!("{}", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: Some(startup_id), error: msg,
                                    });
                                    return;
                                }
                            }

                            let req = tonic::Request::new(StepReplayRequest {
                                request_id: String::new(),
                                token: run_token.clone(),
                            });
                            match run_client.step_replay(req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        let msg = format!("LoadAndStep StepReplay: {} {}", inner.error_code, inner.error_message);
                                        error!("{}", msg);
                                        let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                            startup_id: Some(startup_id), error: msg,
                                        });
                                    } else {
                                        info!("LoadAndStep: step ok, state={:?}", inner.current_state);
                                    }
                                }
                                Err(e) => {
                                    let msg = format!("LoadAndStep: step_replay failed: {}", e);
                                    error!("{}", msg);
                                    let _ = run_status_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: Some(startup_id), error: msg,
                                    });
                                }
                            }
                        });
                    }
                    TransportCommand::RunStrategy { strategy_file, config, startup_id } => {
                        let mut run_client = client.clone();
                        let run_token = token.clone();
                        let run_catalog = catalog_path.clone();
                        let run_status_tx = status_tx.clone();
                        tokio::spawn(async move {
                            let strategy_file_str = strategy_file.to_string_lossy().to_string();

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
                        let mut sem_client = client.clone();
                        let sem_token = token.clone();
                        let sem_seq = std::sync::Arc::clone(&mode_seq);
                        let sem_gate = std::sync::Arc::clone(&mode_gate);
                        let my_seq =
                            sem_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        tokio::spawn(async move {
                            let _guard = sem_gate.lock().await;
                            if sem_seq.load(std::sync::atomic::Ordering::SeqCst) != my_seq {
                                info!(
                                    "SetExecutionMode({}) superseded by a newer switch; dropping (seq={})",
                                    mode.as_wire_str(),
                                    my_seq
                                );
                                return;
                            }
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
                    TransportCommand::ForceAccountSnapshot => {
                        let mut fas_client = client.clone();
                        let fas_token = token.clone();
                        tokio::spawn(async move {
                            let req = tonic::Request::new(ForceAccountSnapshotRequest {
                                token: fas_token,
                            });
                            match fas_client.force_account_snapshot(req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if inner.success {
                                        info!("ForceAccountSnapshot accepted; awaiting AccountEvent on stream");
                                    } else {
                                        error!(
                                            "ForceAccountSnapshot rejected: error_code={}",
                                            inner.error_code
                                        );
                                    }
                                }
                                Err(e) => error!("ForceAccountSnapshot failed: {}", e),
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
                                            strategy_id: ev.strategy_id.unwrap_or_default(),
                                        });
                                    } else {
                                        warn!("PlaceOrder ok but no order_event returned: {}", instrument_id);
                                        let _ = status_tx.send(BackendStatusUpdate::OrderNotice {
                                            message: "発注応答が不完全です — venue で注文状態を確認してください".to_string(),
                                        });
                                    }
                                } else {
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
                            Err(e) => {
                                error!("PlaceOrder failed: {} err={}", instrument_id, e);
                                let _ = status_tx.send(BackendStatusUpdate::OrderNotice {
                                    message: "通信エラー — venue で注文状態を確認してください (発注)".to_string(),
                                });
                            }
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
                            Err(e) => {
                                error!("CancelOrder failed: order_id={} err={}", order_id, e);
                                let _ = status_tx.send(BackendStatusUpdate::OrderNotice {
                                    message: "通信エラー — venue で注文状態を確認してください (取消)".to_string(),
                                });
                            }
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
                                        let _ = status_tx.send(BackendStatusUpdate::OrderNotice {
                                            message: "発注応答が不完全です — venue で注文状態を確認してください".to_string(),
                                        });
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
                            Err(e) => {
                                error!(
                                    "ModifyOrder failed: client_order_id={} err={}",
                                    client_order_id, e
                                );
                                let _ = status_tx.send(BackendStatusUpdate::OrderNotice {
                                    message: "通信エラー — venue で注文状態を確認してください (訂正)".to_string(),
                                });
                            }
                        }
                    }
                    TransportCommand::SubmitSecret { request_id, secret } => {
                        let req = tonic::Request::new(engine::SubmitSecretReq {
                            token: token.clone(),
                            request_id: request_id.clone(),
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
                                    let _ = status_tx.send(BackendStatusUpdate::SecretSubmitFailed {
                                        error_code: inner.error_code,
                                    });
                                }
                            }
                            Err(e) => error!("SubmitSecret failed: request_id={} err={}", request_id, e),
                        }
                    }
                    TransportCommand::GetOrders { venue } => {
                        seed_orders_from_backend(&mut client, &token, venue, &status_tx, false).await;
                    }
                    TransportCommand::StartLiveAuto {
                        instrument_id,
                        venue,
                        strategy_file,
                    } => {
                        let mut c = client.clone();
                        let t = token.clone();
                        let run_failed_tx = status_tx.clone();
                        tokio::spawn(async move {
                            let strategy_file_str = strategy_file.to_string_lossy().to_string();
                            let register_req = tonic::Request::new(RegisterLiveStrategyReq {
                                token: t.clone(),
                                request_id: String::new(),
                                strategy_file: strategy_file_str,
                                expected_sha256: String::new(),
                            });

                            let strategy_id = match c.register_live_strategy(register_req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if let Some(msg) = build_register_reject_message(
                                        inner.success,
                                        &inner.error_code,
                                        &inner.error_message,
                                        &instrument_id,
                                        &venue,
                                    ) {
                                        error!("{}", msg);
                                        let _ = run_failed_tx.send(BackendStatusUpdate::RunFailed {
                                            startup_id: None,
                                            error: msg,
                                        });
                                        return;
                                    }
                                    inner.strategy_id
                                }
                                Err(e) => {
                                    let msg = format!(
                                        "RegisterLiveStrategy failed: instrument_id={} venue={} err={}",
                                        instrument_id, venue, e
                                    );
                                    error!("{}", msg);
                                    let _ = run_failed_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: None,
                                        error: msg,
                                    });
                                    return;
                                }
                            };

                            let safety_limits: SafetyLimits =
                                default_live_auto_safety_limits(&instrument_id);
                            let start_req = tonic::Request::new(StartLiveStrategyReq {
                                token: t,
                                request_id: String::new(),
                                strategy_id: strategy_id.clone(),
                                instrument_id: instrument_id.clone(),
                                venue: venue.clone(),
                                params: Default::default(),
                                safety_limits: Some(safety_limits),
                            });

                            match c.start_live_strategy(start_req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if let Some(msg) = build_start_reject_message(
                                        inner.success,
                                        &inner.error_code,
                                        &inner.error_message,
                                        &strategy_id,
                                        &instrument_id,
                                        &venue,
                                    ) {
                                        error!("{}", msg);
                                        let _ = run_failed_tx.send(BackendStatusUpdate::RunFailed {
                                            startup_id: None,
                                            error: msg,
                                        });
                                    }
                                }
                                Err(e) => {
                                    let msg = format!(
                                        "StartLiveStrategy failed: strategy_id={} instrument_id={} venue={} err={}",
                                        strategy_id, instrument_id, venue, e
                                    );
                                    error!("{}", msg);
                                    let _ = run_failed_tx.send(BackendStatusUpdate::RunFailed {
                                        startup_id: None,
                                        error: msg,
                                    });
                                }
                            }
                        });
                    }
                    TransportCommand::PauseLiveStrategy { run_id } => {
                        let mut c = client.clone();
                        let t = token.clone();
                        tokio::spawn(async move {
                            let req = tonic::Request::new(PauseLiveStrategyReq {
                                token: t,
                                request_id: String::new(),
                                run_id: run_id.clone(),
                            });
                            match c.pause_live_strategy(req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        error!(
                                            "PauseLiveStrategy rejected: run_id={} error_code={}",
                                            run_id, inner.error_code
                                        );
                                    }
                                }
                                Err(e) => error!("PauseLiveStrategy failed: run_id={run_id} err={e}"),
                            }
                        });
                    }
                    TransportCommand::ResumeLiveStrategy { run_id } => {
                        let mut c = client.clone();
                        let t = token.clone();
                        tokio::spawn(async move {
                            let req = tonic::Request::new(ResumeLiveStrategyReq {
                                token: t,
                                request_id: String::new(),
                                run_id: run_id.clone(),
                            });
                            match c.resume_live_strategy(req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        error!(
                                            "ResumeLiveStrategy rejected: run_id={} error_code={}",
                                            run_id, inner.error_code
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("ResumeLiveStrategy failed: run_id={run_id} err={e}")
                                }
                            }
                        });
                    }
                    TransportCommand::StopLiveStrategy { run_id } => {
                        let mut c = client.clone();
                        let t = token.clone();
                        tokio::spawn(async move {
                            let req = tonic::Request::new(StopLiveStrategyReq {
                                token: t,
                                request_id: String::new(),
                                run_id: run_id.clone(),
                            });
                            match c.stop_live_strategy(req).await {
                                Ok(r) => {
                                    let inner = r.into_inner();
                                    if !inner.success {
                                        error!(
                                            "StopLiveStrategy rejected: run_id={} error_code={}",
                                            run_id, inner.error_code
                                        );
                                    }
                                }
                                Err(e) => error!("StopLiveStrategy failed: run_id={run_id} err={e}"),
                            }
                        });
                    }
                    TransportCommand::GetOrdersAndReconcile { venue } => {
                        seed_orders_from_backend(&mut client, &token, venue, &status_tx, true).await;
                    }
                                }
                            }

                            let request = tonic::Request::new(GetStateRequest {
                                token: token.clone(),
                            });

                            match tokio::time::timeout(tokio::time::Duration::from_secs(5), client.get_state(request)).await {
                    Ok(Ok(response)) => {
                        let json_data = response.into_inner().json_data;
                        match serde_json::from_str::<BackendTradingState>(&json_data) {
                            Ok(state) => {
                                if state.venue_state != prev_venue {
                                    if let Some(ref s) = state.venue_state {
                                        match parse_venue_state(s) {
                                            Some(vs) => {
                                                let _ = status_tx.send(BackendStatusUpdate::VenueChanged {
                                                    state: vs,
                                                    venue_id: state.venue_id.clone(),
                                                    instruments_loaded: state.instruments_loaded.unwrap_or(0),
                                                });
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
                                let _ = status_tx.send(BackendStatusUpdate::LastPricesUpdated {
                                    prices: state.last_prices.clone(),
                                });
                                let _ = state_tx.send(state);
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
                                warn!("GetState timed out (backend busy), will retry next poll");
                            }
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
                        } => {}
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Private helpers (all transport-specific)
// ---------------------------------------------------------------------------

/// Map a proto `engine::backend_event::Payload` to our internal `BackendEvent` enum.
/// Shared between `GrpcTransport` (stream decode) and `RustEventSink` (inproc push).
fn map_backend_event_payload(payload: engine::backend_event::Payload) -> BackendEvent {
    match payload {
        engine::backend_event::Payload::SecretRequired(p) => BackendEvent::SecretRequired {
            request_id: p.request_id,
            venue: p.venue,
            kind: p.kind,
            purpose: p.purpose,
        },
        engine::backend_event::Payload::OrderEvent(p) => BackendEvent::OrderEvent {
            order_id: p.order_id,
            venue_order_id: p.venue_order_id,
            client_order_id: p.client_order_id,
            status: p.status,
            filled_qty: p.filled_qty,
            avg_price: p.avg_price,
            ts_ms: p.ts_ms,
            strategy_id: p.strategy_id.unwrap_or_default(),
        },
        engine::backend_event::Payload::AccountEvent(p) => BackendEvent::AccountEvent {
            cash: p.cash,
            buying_power: p.buying_power,
            positions: p
                .positions
                .into_iter()
                .map(|pos| AccountPosition {
                    symbol: pos.symbol,
                    qty: pos.qty,
                    avg_price: pos.avg_price,
                    unrealized_pnl: pos.unrealized_pnl,
                })
                .collect(),
            ts_ms: p.ts_ms,
        },
        engine::backend_event::Payload::VenueLogoutDetected(p) => {
            BackendEvent::VenueLogoutDetected { venue: p.venue }
        }
        engine::backend_event::Payload::LiveStrategyEvent(p) => {
            BackendEvent::LiveStrategyEvent {
                run_id: p.run_id,
                strategy_id: p.strategy_id,
                status: p.status,
                ts_ms: p.ts_ms,
            }
        }
        engine::backend_event::Payload::SafetyRailViolation(p) => {
            BackendEvent::SafetyRailViolation {
                run_id: p.run_id,
                kind: p.kind,
                detail: p.detail,
                ts_ms: p.ts_ms,
            }
        }
        engine::backend_event::Payload::StrategyLogMessage(p) => {
            BackendEvent::StrategyLogMessage {
                run_id: p.run_id,
                level: p.level,
                message: p.message,
                ts_ms: p.ts_ms,
            }
        }
        engine::backend_event::Payload::LiveStrategyTelemetry(p) => {
            BackendEvent::LiveStrategyTelemetry {
                run_id: p.run_id,
                strategy_id: p.strategy_id,
                realized_pnl: p.realized_pnl,
                unrealized_pnl: p.unrealized_pnl,
                order_count: p.order_count,
                fill_count: p.fill_count,
                ts_ms: p.ts_ms,
            }
        }
        engine::backend_event::Payload::BackendError(p) => BackendEvent::BackendError {
            source: p.source,
            detail: p.detail,
            ts_ms: p.ts_ms,
        },
    }
}

/// Fire `ListInstruments` off the main polling loop and emit the three-part
/// `InstrumentsListStarted` / `InstrumentsListed` / `InstrumentsListFailed`
/// sequence.
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

/// §3.8 / S6: GetOrders RPC を撃って full LiveOrder 行で OrderPanel を seed する
/// 共通処理。`reconcile == true`（auto-restart 経路）のときのみ、seed 後に
/// id-diff 用の OrdersReconciled を送る。
async fn seed_orders_from_backend(
    client: &mut DataEngineClient<tonic::transport::Channel>,
    token: &str,
    venue: String,
    status_tx: &mpsc::UnboundedSender<BackendStatusUpdate>,
    reconcile: bool,
) {
    let req = tonic::Request::new(engine::GetOrdersReq {
        token: token.to_owned(),
        venue,
    });
    let seeded = match client.get_orders(req).await {
        Ok(r) => {
            let inner = r.into_inner();
            if !inner.success {
                info!(
                    "GetOrders reconcile: backend reports none ({})",
                    inner.error_code
                );
            }
            if let Some(notice) = get_orders_notice(&inner.error_code) {
                let _ = status_tx.send(notice);
            }
            inner
                .orders
                .into_iter()
                .map(|o| LiveOrder {
                    client_order_id: o.client_order_id,
                    venue_order_id: o.venue_order_id,
                    symbol: o.symbol,
                    side: o.side,
                    qty: o.qty,
                    price: o.price,
                    status: o.status,
                    filled_qty: o.filled_qty,
                    avg_price: o.avg_price,
                    ts_ms: o.ts_ms,
                    strategy_id: o.strategy_id.unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        }
        Err(e) => {
            warn!("GetOrders failed during reconcile: {}", e);
            Vec::new()
        }
    };
    let reconcile_ids = reconcile_ids_for_seed(&seeded, reconcile);
    let _ = status_tx.send(BackendStatusUpdate::OrdersSeeded { orders: seeded });
    if let Some(backend_client_order_ids) = reconcile_ids {
        let _ = status_tx.send(BackendStatusUpdate::OrdersReconciled {
            backend_client_order_ids,
        });
    }
}

/// Backend-events reconnect backoff: wait for either a lifecycle change or a
/// 500ms timer before retrying the stream.  Returns `false` when the supervisor's
/// watch sender was dropped (app exit) — caller should return.
async fn events_reconnect_backoff(rx: &mut tokio::sync::watch::Receiver<BackendLifecycle>) -> bool {
    tokio::select! {
        changed = rx.changed() => changed.is_ok(),
        _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => true,
    }
}

/// SELECTIVE reconnect flush (§3.8): keep only the reconcile primitives
/// (`GetOrdersAndReconcile`, `FetchAvailableInstruments`), drop all stale
/// session-scoped intents.
pub fn flush_stale_transport_commands(
    drained: impl IntoIterator<Item = TransportCommand>,
) -> std::collections::VecDeque<TransportCommand> {
    drained.into_iter().filter(is_reconcile_command).collect()
}

fn is_reconcile_command(cmd: &TransportCommand) -> bool {
    matches!(
        cmd,
        TransportCommand::GetOrdersAndReconcile { .. }
        | TransportCommand::FetchAvailableInstruments { .. }
    )
}

/// Parse `VenueState` from backend string (e.g. `"CONNECTED"`).
fn parse_venue_state(s: &str) -> Option<VenueState> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

/// Parse `ExecutionMode` from backend string (e.g. `"LiveManual"`).
fn parse_execution_mode(s: &str) -> Option<ExecutionMode> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

pub fn parse_replay_granularity(s: &str) -> Result<i32, String> {
    match s {
        "Daily" => Ok(ReplayGranularity::Daily as i32),
        "Minute" => Ok(ReplayGranularity::Minute as i32),
        other => Err(format!("unknown granularity: {:?}", other)),
    }
}

// ---------------------------------------------------------------------------
// RustEventSink — PyO3 callable that Python uses to push BackendEvents (Phase 3)
// ---------------------------------------------------------------------------

/// A Python-callable object that forwards serialised `engine.BackendEvent` proto
/// bytes into the Rust tokio mpsc channel.  Created once per inproc session and
/// registered on `DataEngine` via `set_rust_event_sink(sink)`.
///
/// GIL design: `push()` is called from Python (GIL already held by the caller —
/// typically the live-loop asyncio thread).  We decode the proto while holding the
/// GIL (cheap, in-memory) and send it on the unbounded channel without releasing
/// the GIL — the send is non-blocking, so there is nothing to overlap by calling
/// `py.allow_threads`.
#[pyo3::pyclass]
struct RustEventSink {
    event_tx: mpsc::UnboundedSender<BackendEvent>,
}

#[pyo3::pymethods]
impl RustEventSink {
    /// Called from Python: `sink.push(event.SerializeToString())`
    fn push(&self, data: &[u8]) -> pyo3::PyResult<()> {
        use prost::Message as _;
        let proto = engine::BackendEvent::decode(data)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        if let Some(payload) = proto.payload {
            let _ = self.event_tx.send(map_backend_event_payload(payload));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RustBacktestSink — PyO3 callable for BacktestEngine → Rust bridge (issue #68)
// ---------------------------------------------------------------------------

/// Python-callable sink passed to `InprocLiveServer.start_nautilus_replay()`.
/// Python calls `push_bar(state_json)` for every bar processed by BacktestEngine;
/// the JSON is forwarded as an `InProcResp::StateJson` so the chart updates in
/// real-time without polling.
///
/// GIL design: all methods are called from the Python worker thread (GIL held by
/// the caller). The channel sends are non-blocking (unbounded), so we never need
/// `py.allow_threads`.
#[pyo3::pyclass]
struct RustBacktestSink {
    resp_tx: mpsc::UnboundedSender<InProcResp>,
    startup_id: u64,
}

#[pyo3::pymethods]
impl RustBacktestSink {
    /// Called once per bar: `sink.push_bar(state_json_str)`.
    /// `state_json_str` must deserialise as `BackendTradingState`.
    fn push_bar(&self, state_json: &str) -> pyo3::PyResult<()> {
        let _ = self.resp_tx.send(InProcResp::StateJson(state_json.to_string()));
        Ok(())
    }

    /// Slice 3: deserialise the order JSON from GuiBridgeActor and forward as
    /// BackendStatusUpdate::OrderSeeded so the UI's LiveOrders table populates.
    fn push_order(&self, json: &str) -> pyo3::PyResult<()> {
        #[derive(serde::Deserialize)]
        struct RawOrder {
            symbol: String,
            client_order_id: String,
            venue_order_id: String,
            strategy_id: String,
            side: String,
            status: String,
            qty: f64,
            price: f64,
            timestamp_ms: i64,
        }
        match serde_json::from_str::<RawOrder>(json) {
            Ok(o) => {
                let _ = self.resp_tx.send(InProcResp::Status(
                    crate::trading::BackendStatusUpdate::OrderSeeded {
                        client_order_id: o.client_order_id,
                        venue_order_id: o.venue_order_id,
                        symbol: o.symbol,
                        side: o.side,
                        qty: o.qty,
                        price: Some(o.price),
                        status: o.status,
                        filled_qty: o.qty,
                        avg_price: o.price,
                        ts_ms: o.timestamp_ms,
                        strategy_id: o.strategy_id,
                    },
                ));
            }
            Err(e) => {
                warn!("[sink] push_order deserialise failed: {}", e);
            }
        }
        Ok(())
    }

    /// Slice 4: deserialise the portfolio JSON from GuiBridgeActor and forward as
    /// BackendStatusUpdate::PortfolioLoaded so the buying-power panel populates.
    fn push_portfolio(&self, json: &str) -> pyo3::PyResult<()> {
        #[derive(serde::Deserialize)]
        struct RawPosition {
            symbol: String,
            qty: f64,
            avg_price: f64,
        }
        #[derive(serde::Deserialize)]
        struct RawPortfolio {
            buying_power: f64,
            equity: f64,
            positions: Vec<RawPosition>,
        }
        match serde_json::from_str::<RawPortfolio>(json) {
            Ok(p) => {
                let positions = p
                    .positions
                    .into_iter()
                    .map(|pos| crate::trading::PortfolioPosition {
                        symbol: pos.symbol,
                        qty: pos.qty as i64,
                        avg_price: pos.avg_price,
                        unrealized_pnl: 0.0,
                    })
                    .collect();
                let _ = self.resp_tx.send(InProcResp::Status(
                    crate::trading::BackendStatusUpdate::PortfolioLoaded {
                        buying_power: p.buying_power,
                        cash: p.buying_power,
                        equity: p.equity,
                        positions,
                        orders: vec![],
                    },
                ));
            }
            Err(e) => {
                warn!("[sink] push_portfolio deserialise failed: {}", e);
            }
        }
        Ok(())
    }

    /// Placeholder for Slice 6 (run-result telemetry).
    fn push_telemetry(&self, _json: &str) -> pyo3::PyResult<()> {
        Ok(())
    }

    /// Called once when BacktestEngine.run() completes successfully.
    fn push_run_complete(&self, run_id: &str, summary_json: &str) -> pyo3::PyResult<()> {
        let _ = self.resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunComplete {
            startup_id: Some(self.startup_id),
            run_id: run_id.to_string(),
            summary_json: summary_json.to_string(),
        }));
        Ok(())
    }

    /// Called from the backtest background thread when the run fails (Slice 2).
    /// Mirrors the synchronous RunFailed path in inproc_dispatch so the UI shows
    /// the error even though start_nautilus_replay() returned immediately.
    fn push_run_failed(&self, error: &str) -> pyo3::PyResult<()> {
        let _ = self.resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
            startup_id: Some(self.startup_id),
            error: error.to_string(),
        }));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// InProcTransport — PyO3 direct call implementation (Phase 2)
// ---------------------------------------------------------------------------

/// Response messages from the Python dedicated thread back to the tokio world.
enum InProcResp {
    StateJson(String),
    Status(BackendStatusUpdate),
}

pub struct InProcTransport {
    pub catalog_path: Option<String>,
    pub max_history_len: usize,
    /// Directory inserted at sys.path[0] so `import engine` resolves.
    pub python_engine_path: String,
    pub poll_interval_ms: u64,
    /// Live venue id (mirrors LIVE_VENUE env var) forwarded to InprocLiveServer.
    pub live_venue_id: Option<String>,
}

impl BackendTransport for InProcTransport {
    fn run(
        self: Box<Self>,
        mut transport_rx: mpsc::UnboundedReceiver<TransportCommand>,
        state_tx: mpsc::UnboundedSender<BackendTradingState>,
        status_tx: mpsc::UnboundedSender<BackendStatusUpdate>,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
        mut lifecycle_rx: tokio::sync::watch::Receiver<BackendLifecycle>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        let catalog_path = self.catalog_path;
        let max_history_len = self.max_history_len;
        let python_engine_path = self.python_engine_path;
        let poll_interval_ms = self.poll_interval_ms;
        let live_venue_id = self.live_venue_id;

        Box::pin(async move {
            // Supervisor emits Ready immediately in inproc mode; wait for it.
            if lifecycle_rx
                .wait_for(|s| matches!(s, BackendLifecycle::Ready))
                .await
                .is_err()
            {
                return;
            }

            // Bridge: tokio UnboundedReceiver → std::sync::mpsc (Python thread is synchronous).
            // Unbounded variant: Sender::send() never blocks, so the tokio worker thread is
            // never parked even when the Python thread is busy processing a long Python call.
            let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TransportCommand>();
            tokio::spawn(async move {
                while let Some(cmd) = transport_rx.recv().await {
                    if cmd_tx.send(cmd).is_err() {
                        break;
                    }
                }
            });

            // Response channel: Python thread → tokio world.
            let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<InProcResp>();

            // Spawn the dedicated Python thread.
            if let Err(e) = std::thread::Builder::new()
                .name("python-engine".to_string())
                .spawn(move || {
                    inproc_python_worker(
                        cmd_rx,
                        resp_tx,
                        event_tx,
                        catalog_path,
                        max_history_len,
                        python_engine_path,
                        poll_interval_ms,
                        live_venue_id,
                    );
                })
            {
                error!("[inproc] failed to spawn Python thread: {}", e);
                let _ = status_tx.send(BackendStatusUpdate::Connected(false));
                let _ = status_tx.send(BackendStatusUpdate::Error(format!(
                    "InProc thread spawn failed: {}", e
                )));
                return;
            }

            // State diffing state (mirrors GrpcTransport inner loop).
            let mut prev_venue: Option<String> = None;
            let mut prev_mode: Option<String> = None;
            let mut prev_configured_venue: Option<Option<String>> = None;

            // Response handler — runs until Python thread exits (resp_rx closes).
            while let Some(resp) = resp_rx.recv().await {
                match resp {
                    InProcResp::StateJson(json) => {
                        match serde_json::from_str::<BackendTradingState>(&json) {
                            Ok(state) => {
                                if state.venue_state != prev_venue {
                                    if let Some(ref s) = state.venue_state {
                                        match parse_venue_state(s) {
                                            Some(vs) => {
                                                let _ = status_tx.send(BackendStatusUpdate::VenueChanged {
                                                    state: vs,
                                                    venue_id: state.venue_id.clone(),
                                                    instruments_loaded: state.instruments_loaded.unwrap_or(0),
                                                });
                                            }
                                            None => warn!("[inproc] unknown venue_state: {:?}", s),
                                        }
                                    }
                                    prev_venue = state.venue_state.clone();
                                }
                                if state.execution_mode != prev_mode {
                                    if let Some(ref m) = state.execution_mode {
                                        match parse_execution_mode(m) {
                                            Some(em) => {
                                                let _ = status_tx.send(BackendStatusUpdate::ExecutionModeChanged { mode: em });
                                            }
                                            None => warn!("[inproc] unknown execution_mode: {:?}", m),
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
                                let _ = status_tx.send(BackendStatusUpdate::LastPricesUpdated {
                                    prices: state.last_prices.clone(),
                                });
                                let _ = status_tx.send(BackendStatusUpdate::Connected(true));
                                let _ = state_tx.send(state);
                            }
                            Err(e) => {
                                error!("[inproc] state JSON parse error: {}; dropping state", e);
                            }
                        }
                    }
                    InProcResp::Status(upd) => {
                        let _ = status_tx.send(upd);
                    }
                }
            }
            // Python thread exited — signal disconnected.
            let _ = status_tx.send(BackendStatusUpdate::Connected(false));
        })
    }
}

/// The dedicated Python thread.  GIL is acquired only for the duration of each
/// Python call; between calls the thread blocks on `cmd_rx.recv_timeout` with no
/// GIL held, so other Python threads (if any) can run freely.
fn inproc_python_worker(
    cmd_rx: std::sync::mpsc::Receiver<TransportCommand>,
    resp_tx: mpsc::UnboundedSender<InProcResp>,
    event_tx: mpsc::UnboundedSender<BackendEvent>,
    catalog_path: Option<String>,
    max_history_len: usize,
    python_engine_path: String,
    poll_interval_ms: u64,
    live_venue_id: Option<String>,
) {
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList};

    info!("[inproc] Python worker thread starting");

    // Initialize DataEngine — hold GIL only during setup.
    let engine: Py<PyAny> = match Python::with_gil(|py| -> PyResult<Py<PyAny>> {
        // Add engine package directory to sys.path.
        let sys = py.import_bound("sys")?;
        let path = sys.getattr("path")?;
        let path_list = path.downcast::<PyList>()?;
        path_list.insert(0, &python_engine_path)?;

        // Windows: disable bytecode writes. Python's FileFinder re-scans PYTHON_ENGINE_PATH
        // between with_gil blocks when __pycache__ directories exist (the dir mtime changes
        // trigger cache invalidation). The re-scan calls Windows filesystem APIs that fail
        // with WinError 6714 (ERROR_RM_NOT_CONNECTED) when a TxF filter driver is active
        // (Windows Defender / VSS). By disabling bytecode writes, no new __pycache__ entries
        // are created, so directory mtimes stay unchanged and the re-scan never triggers.
        // Prerequisite: delete python/**/__pycache__ before the first run (the startup
        // script scripts/run_inproc.ps1 handles this automatically).
        sys.setattr("dont_write_bytecode", true)?;

        let module = py.import_bound("engine.core")?;
        let cls = module.getattr("DataEngine")?;

        let kwargs = PyDict::new_bound(py);
        if let Some(ref cp) = catalog_path {
            kwargs.set_item("nautilus_catalog_path", cp)?;
        }
        kwargs.set_item("max_history_len", max_history_len)?;

        let engine = cls.call((), Some(&kwargs))?;
        Ok(engine.into())
    }) {
        Ok(e) => {
            info!("[inproc] DataEngine initialized");
            e
        }
        Err(e) => {
            error!("[inproc] DataEngine init failed: {}", e);
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::Error(format!(
                "InProc DataEngine init failed: {}", e
            ))));
            return;
        }
    };

    // Phase 3: register RustEventSink on the DataEngine so that Python's
    // publish_backend_event() forwards live events to our tokio channel.
    if let Err(e) = Python::with_gil(|py| -> pyo3::PyResult<()> {
        let sink = pyo3::Py::new(py, RustEventSink { event_tx })?;
        engine.bind(py).call_method1("set_rust_event_sink", (sink,))?;
        Ok(())
    }) {
        error!("[inproc] RustEventSink registration failed: {}", e);
    } else {
        info!("[inproc] RustEventSink registered on DataEngine");
    }

    // Phase 4: instantiate InprocLiveServer wrapping GrpcDataEngineServer so that
    // live commands (VenueLogin, PlaceOrder, etc.) route directly to Python.
    let live_server: Py<PyAny> = match Python::with_gil(|py| -> PyResult<Py<PyAny>> {
        use pyo3::types::PyDict;
        let module = py.import_bound("engine.inproc_server")?;
        let cls = module.getattr("InprocLiveServer")?;
        let kwargs = PyDict::new_bound(py);
        if let Some(ref vid) = live_venue_id {
            kwargs.set_item("live_venue_id", vid)?;
        }
        let srv = cls.call((engine.bind(py),), Some(&kwargs))?;
        Ok(srv.into())
    }) {
        Ok(s) => {
            info!("[inproc] InprocLiveServer initialized (live_venue_id={:?})", live_venue_id);
            // #2 fix: startup is complete only once live commands are available.
            // Route through the pure helper so its regression test guards this arm.
            for update in inproc_startup_status_sequence(/*live_server_ok=*/ true) {
                let _ = resp_tx.send(InProcResp::Status(update));
            }
            // #7 fix: mirror the gRPC transport's initial ListInstruments on startup
            // (see fire_list_instruments at the connect path) so the instrument-dependent
            // UI is seeded in in-proc mode too. The TickersSource is the single source of
            // truth in inproc_startup_instrument_fetch(); reuse the dispatch arm so the
            // emitted status sequence (InstrumentsListStarted/Listed/Failed) is identical.
            if let Some(source) = inproc_startup_instrument_fetch() {
                inproc_dispatch(
                    &engine,
                    &s,
                    TransportCommand::ListInstruments { source },
                    &resp_tx,
                    &catalog_path,
                );
            }
            s
        }
        Err(e) => {
            error!("[inproc] InprocLiveServer init failed: {}; aborting worker", e);
            // #2 fix: do NOT continue with a None sentinel. Report disconnect + error and exit.
            // Route through the pure helper so its regression test guards this arm.
            for update in inproc_startup_status_sequence(/*live_server_ok=*/ false) {
                let _ = resp_tx.send(InProcResp::Status(update));
            }
            return;
        }
    };

    let poll_duration = std::time::Duration::from_millis(poll_interval_ms);

    loop {
        // Wait for a command; on timeout, poll GetState.  GIL is NOT held here.
        let cmd = cmd_rx.recv_timeout(poll_duration);

        match cmd {
            Ok(cmd) => {
                inproc_dispatch(&engine, &live_server, cmd, &resp_tx, &catalog_path);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                inproc_poll_state(&live_server, &resp_tx);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                info!("[inproc] command channel closed; Python worker exiting");
                // #64 finding #6: tear down the live loop thread / runner /
                // account-sync that InprocLiveServer wraps before the worker
                // exits, otherwise they leak. live_server only exists on the
                // successful-init path (init failure early-returns above).
                Python::with_gil(|py| {
                    if let Err(e) = live_server.bind(py).call_method0("close") {
                        error!("[inproc] live_server.close() failed: {}", e);
                    }
                });
                break;
            }
        }
    }
}

/// Pure helper: the status updates the InProc worker emits **after DataEngine init
/// succeeds**, gated on whether InprocLiveServer init then succeeded. The worker
/// calls this from both live-init arms so the helper is the single source of truth
/// for the live-init status sequence (issue #64 finding #2 regression guard).
///
/// DataEngine-init failure is NOT modeled here: it early-returns inline with an
/// `Error("InProc DataEngine init failed: {e}")` that carries the exception detail.
fn inproc_startup_status_sequence(live_server_ok: bool) -> Vec<BackendStatusUpdate> {
    if !live_server_ok {
        // #2 fix: live commands are unavailable, so we are NOT truly connected.
        return vec![
            BackendStatusUpdate::Connected(false),
            BackendStatusUpdate::Error("InprocLiveServer init failed".to_string()),
        ];
    }
    vec![
        BackendStatusUpdate::Connected(true),
        BackendStatusUpdate::Running(true),
    ]
}

/// Pure helper: the instrument-list fetch the InProc worker performs at startup,
/// expressed as its `TickersSource` (or `None` if it performs no startup fetch).
///
/// The gRPC transport fires `fire_list_instruments(.., TickersSource::ReplayCatalogFallback, ..)`
/// once on connect (see `:185`), so the UI is seeded with an instrument universe at
/// startup. The InProc worker must do the same. (issue #64 finding #7.)
///
/// NOTE: the InProc worker now performs the same startup instrument fetch as the
/// gRPC transport — it fires ListInstruments(ReplayCatalogFallback) once on startup
/// (see the `Ok(s) =>` arm after InprocLiveServer init). This helper is the single
/// source of truth the worker consults to decide that fetch. (issue #64 finding #7 fixed.)
fn inproc_startup_instrument_fetch() -> Option<crate::trading::TickersSource> {
    Some(crate::trading::TickersSource::ReplayCatalogFallback)
}

/// Poll state via InprocLiveServer.get_state_json() (live mode returns price/depth enriched state).
fn inproc_poll_state(live_server: &Py<PyAny>, resp_tx: &mpsc::UnboundedSender<InProcResp>) {
    use pyo3::prelude::*;

    let state_json = Python::with_gil(|py| -> Result<String, ()> {
        live_server.bind(py).call_method0("get_state_json")
            .map_err(|_| ())?
            .extract::<String>().map_err(|_| ())
    });

    match inproc_poll_state_outcome(state_json) {
        PollStateOutcome::Forward(json) => {
            let _ = resp_tx.send(InProcResp::StateJson(json));
        }
        PollStateOutcome::LogAndSkip => {
            warn!("InProc get_state_json() poll failed; skipping this tick");
        }
    }
}

/// Classify the outcome of an InProc `get_state_json()` poll.
/// `Forward(json)` = success → send StateJson. `LogAndSkip` = a call/extract
/// failure is surfaced (warn!) before skipping, matching the gRPC GetState
/// path which always makes a failure visible (Error status / warn!; see
/// :1182-1196). (issue #64 finding #3 fixed.)
#[derive(Debug, PartialEq, Eq)]
enum PollStateOutcome {
    Forward(String),
    LogAndSkip,
}

/// Pure classifier for `inproc_poll_state`. `Err(())` collapses either a
/// `get_state_json` call failure or a non-String extract failure.
///
/// A poll failure is surfaced (`LogAndSkip`) so `inproc_poll_state` can
/// `warn!` before skipping, rather than dropping it silently. (issue #64
/// finding #3 fixed.)
fn inproc_poll_state_outcome(state_json: Result<String, ()>) -> PollStateOutcome {
    match state_json {
        Ok(json) => PollStateOutcome::Forward(json),
        Err(()) => PollStateOutcome::LogAndSkip,
    }
}

/// Call a zero-argument replay method that returns `(bool, str | None)`.
fn inproc_call_replay(
    engine: &Py<PyAny>,
    method: &str,
) -> (bool, Option<String>) {
    use pyo3::prelude::*;

    Python::with_gil(|py| {
        match engine.bind(py).call_method0(method) {
            Ok(val) => val
                .extract::<(bool, Option<String>)>()
                .unwrap_or((false, Some(format!("{}: extract failed", method)))),
            Err(e) => (false, Some(format!("{}: PyO3 error: {}", method, e))),
        }
    })
}

/// Call `engine.set_replay_speed(multiplier)`.
fn inproc_set_speed(engine: &Py<PyAny>, multiplier: u32) {
    use pyo3::prelude::*;

    Python::with_gil(|py| {
        if let Err(e) = engine.bind(py).call_method1("set_replay_speed", (multiplier,)) {
            warn!("[inproc] set_replay_speed error: {}", e);
        }
    });
}

/// Call `engine.load_replay_data(...)`.
fn inproc_load_replay_data(
    engine: &Py<PyAny>,
    instrument_ids: &[String],
    start_date: &str,
    end_date: &str,
    granularity: &str,
    catalog_path: Option<&str>,
) -> (bool, Option<String>) {
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList};

    Python::with_gil(|py| {
        let kwargs = PyDict::new_bound(py);
        let py_ids = PyList::new_bound(py, instrument_ids.iter().map(|s| s.as_str()));
        if let Err(e) = kwargs.set_item("instrument_ids", py_ids) {
            return (false, Some(format!("kwargs set_item error: {}", e)));
        }
        let _ = kwargs.set_item("start_date", start_date);
        let _ = kwargs.set_item("end_date", end_date);
        let _ = kwargs.set_item("granularity", granularity);
        if let Some(cp) = catalog_path {
            let _ = kwargs.set_item("catalog_path", cp);
        }

        match engine.bind(py).call_method("load_replay_data", (), Some(&kwargs)) {
            Ok(val) => val
                .extract::<(bool, Option<String>)>()
                .unwrap_or((false, Some("load_replay_data: extract failed".to_string()))),
            Err(e) => (false, Some(format!("load_replay_data PyO3 error: {}", e))),
        }
    })
}

// ---------------------------------------------------------------------------
// InprocLiveServer call helpers (Phase 4)
// ---------------------------------------------------------------------------

/// Call a method on `InprocLiveServer` with kwargs built by `build_kwargs`, then
/// deserialise the returned Python dict via JSON into a `serde_json::Value`.
/// The closure receives a `&Bound<'_, PyDict>` so callers can use `.set_item()`.
fn inproc_live_call<F>(
    live_server: &Py<PyAny>,
    method: &str,
    build_kwargs: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(pyo3::Python<'_>, &pyo3::Bound<'_, pyo3::types::PyDict>) -> pyo3::PyResult<()>,
{
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyDictMethods};

    Python::with_gil(|py| {
        let kwargs = PyDict::new_bound(py);
        build_kwargs(py, &kwargs).map_err(|e| format!("{}: kwargs build error: {}", method, e))?;
        let result = live_server
            .bind(py)
            .call_method(method, (), Some(&kwargs))
            .map_err(|e| format!("{}: PyO3 error: {}", method, e))?;
        inproc_json_dumps(py, result, method)
    })
}

/// Call a method on `InprocLiveServer` with a single positional Python dict arg.
/// The closure builds the dict by calling `.set_item()` on the provided `&Bound<PyDict>`.
#[allow(dead_code)] // used by Slice 2–8; RunStrategy now uses start_nautilus_replay directly
fn inproc_live_call_positional_dict<F>(
    live_server: &Py<PyAny>,
    method: &str,
    build_dict: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(pyo3::Python<'_>, &pyo3::Bound<'_, pyo3::types::PyDict>) -> pyo3::PyResult<()>,
{
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyDictMethods};

    Python::with_gil(|py| {
        let cfg = PyDict::new_bound(py);
        build_dict(py, &cfg).map_err(|e| format!("{}: dict build error: {}", method, e))?;
        let result = live_server
            .bind(py)
            .call_method1(method, (cfg,))
            .map_err(|e| format!("{}: PyO3 error: {}", method, e))?;
        inproc_json_dumps(py, result, method)
    })
}

fn inproc_json_dumps(
    py: pyo3::Python<'_>,
    value: pyo3::Bound<'_, pyo3::PyAny>,
    context: &str,
) -> Result<serde_json::Value, String> {
    use pyo3::prelude::*;
    let json_mod = py.import_bound("json").map_err(|e| format!("import json: {}", e))?;
    let json_str: String = json_mod
        .call_method1("dumps", (value,))
        .map_err(|e| format!("{}: json.dumps: {}", context, e))?
        .extract()
        .map_err(|e| format!("{}: extract json str: {}", context, e))?;
    serde_json::from_str(&json_str).map_err(|e| format!("{}: json parse: {}", context, e))
}

/// Call a no-argument method on InprocLiveServer for backtest Pause/Step/Resume control.
/// Returns true on success so callers can gate follow-up actions (e.g. StepForward
/// must not poll state when the step itself failed).
fn inproc_nautilus_control(live_server: &pyo3::Py<pyo3::PyAny>, method: &str) -> bool {
    use pyo3::prelude::*;
    Python::with_gil(|py| {
        match live_server.bind(py).call_method0(method) {
            Ok(_) => { info!("[inproc] {} ok", method); true }
            Err(e) => { error!("[inproc] {} failed: {}", method, e); false }
        }
    })
}

/// Seed orders from backend via InprocLiveServer.get_orders().
fn inproc_seed_orders(
    live_server: &Py<PyAny>,
    venue: String,
    resp_tx: &mpsc::UnboundedSender<InProcResp>,
    reconcile: bool,
) {
    use pyo3::types::PyDictMethods;
    match inproc_live_call(live_server, "get_orders", |_py, kwargs| {
        kwargs.set_item("venue", &venue)?;
        Ok(())
    }) {
        Ok(r) => {
            let orders: Vec<crate::trading::LiveOrder> = r.get("orders")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|o| {
                    Some(crate::trading::LiveOrder {
                        client_order_id: o.get("client_order_id")?.as_str()?.to_owned(),
                        venue_order_id: o.get("venue_order_id")?.as_str()?.to_owned(),
                        symbol: o.get("symbol")?.as_str()?.to_owned(),
                        side: o.get("side")?.as_str()?.to_owned(),
                        qty: o.get("qty")?.as_f64()?,
                        price: o.get("price").and_then(|v| v.as_f64()),
                        status: o.get("status")?.as_str()?.to_owned(),
                        filled_qty: o.get("filled_qty")?.as_f64()?,
                        avg_price: o.get("avg_price")?.as_f64()?,
                        ts_ms: o.get("ts_ms")?.as_i64()?,
                        strategy_id: o.get("strategy_id").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                    })
                }).collect())
                .unwrap_or_default();
            let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(notice) = get_orders_notice(&ec) {
                let _ = resp_tx.send(InProcResp::Status(notice));
            }
            let reconcile_ids = reconcile_ids_for_seed(&orders, reconcile);
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrdersSeeded { orders }));
            if let Some(ids) = reconcile_ids {
                let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrdersReconciled { backend_client_order_ids: ids }));
            }
        }
        Err(msg) => warn!("[inproc] GetOrders failed: {}", msg),
    }
}

/// Load portfolio from InprocLiveServer.get_portfolio() and emit PortfolioLoaded.
#[allow(dead_code)] // used by Slice 4 (portfolio push); kept for future use
fn inproc_get_portfolio(live_server: &Py<PyAny>, resp_tx: &mpsc::UnboundedSender<InProcResp>) {
    match inproc_live_call(live_server, "get_portfolio", |_py, _kwargs| Ok(())) {
        Ok(r) => {
            if r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                let positions = r.get("positions")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|p| {
                        Some(crate::trading::PortfolioPosition {
                            symbol: p.get("symbol")?.as_str()?.to_owned(),
                            qty: p.get("qty")?.as_i64()?,
                            avg_price: p.get("avg_price")?.as_f64()?,
                            unrealized_pnl: p.get("unrealized_pnl")?.as_f64()?,
                        })
                    }).collect())
                    .unwrap_or_default();
                let orders = r.get("orders")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|o| {
                        Some(crate::trading::PortfolioOrder {
                            symbol: o.get("symbol")?.as_str()?.to_owned(),
                            side: o.get("side")?.as_str()?.to_owned(),
                            qty: o.get("qty")?.as_f64()?,
                            price: o.get("price")?.as_f64()?,
                            status: o.get("status")?.as_str()?.to_owned(),
                            ts_ms: o.get("ts_ms")?.as_i64()?,
                        })
                    }).collect())
                    .unwrap_or_default();
                let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::PortfolioLoaded {
                    buying_power: r.get("buying_power").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    cash: r.get("cash").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    equity: r.get("equity").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    positions,
                    orders,
                }));
            }
        }
        Err(msg) => warn!("[inproc] GetPortfolio failed: {}", msg),
    }
}

/// Convert `SafetyLimits` proto struct to a Python-compatible dict representation.

/// Dispatch a single `TransportCommand` to the appropriate Python call.
fn inproc_dispatch(
    engine: &Py<PyAny>,
    live_server: &Py<PyAny>,
    cmd: TransportCommand,
    resp_tx: &mpsc::UnboundedSender<InProcResp>,
    default_catalog: &Option<String>,
) {
    // Trait imports for Bound<PyDict/PyList> method dispatch in closures below.
    #[allow(unused_imports)]
    use pyo3::types::{PyDictMethods, PyListMethods};
    match cmd {
        // ---------------------------------------------------------------
        // Replay commands — routed to BacktestEngine via threading.Event
        // (Slice 2: live_server.pause/resume/step_backtest() manipulate the
        //  shared threading.Event inside GuiBridgeActor._on_bar() so the
        //  backtest background thread pauses/steps without holding the GIL)
        // ---------------------------------------------------------------
        TransportCommand::Pause => {
            let _ = inproc_nautilus_control(live_server, "pause_backtest");
        }
        TransportCommand::Resume => {
            let _ = inproc_nautilus_control(live_server, "resume_backtest");
        }
        TransportCommand::StepForward => {
            if inproc_nautilus_control(live_server, "step_backtest") {
                inproc_poll_state(live_server, resp_tx);
            }
        }
        TransportCommand::ForceStop => {
            let (ok, err) = inproc_call_replay(engine, "force_stop_replay");
            if ok {
                info!("[inproc] ForceStopReplay ok");
            } else {
                error!("[inproc] ForceStopReplay failed: {:?}", err);
            }
        }
        TransportCommand::SetSpeed(mult) => {
            inproc_set_speed(engine, mult);
            use pyo3::prelude::*;
            Python::with_gil(|py| {
                if let Err(e) = live_server.bind(py).call_method1("set_replay_speed", (mult,)) {
                    warn!("[inproc] live_server.set_replay_speed error: {}", e);
                }
            });
        }
        TransportCommand::LoadAndStep { config, startup_id } => {
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::ReplayStartup {
                startup_id,
                stage: crate::trading::BackendStartupStage::ResettingReplay,
            }));
            let (ok, err) = inproc_call_replay(engine, "force_stop_replay");
            if !ok {
                let msg = format!("LoadAndStep ForceStop: {:?}", err);
                error!("[inproc] {}", msg);
                let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
                    startup_id: Some(startup_id),
                    error: msg,
                }));
                return;
            }

            let granularity = match parse_replay_granularity(&config.granularity) {
                Ok(_) => config.granularity.as_str(),
                Err(msg) => {
                    error!("[inproc] LoadAndStep: {}", msg);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
                        startup_id: Some(startup_id),
                        error: msg,
                    }));
                    return;
                }
            };

            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::ReplayStartup {
                startup_id,
                stage: crate::trading::BackendStartupStage::LoadingData,
            }));

            let (ok, err) = inproc_load_replay_data(
                engine,
                &config.instruments,
                &config.start,
                &config.end,
                granularity,
                default_catalog.as_deref(),
            );
            if !ok {
                let msg = format!("LoadAndStep LoadReplayData: {:?}", err);
                error!("[inproc] {}", msg);
                let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
                    startup_id: Some(startup_id),
                    error: msg,
                }));
                return;
            }
            info!("[inproc] LoadReplayData ok");

            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::ReplayStartup {
                startup_id,
                stage: crate::trading::BackendStartupStage::WaitingForFirstTick,
            }));

            let (ok, err) = inproc_call_replay(engine, "step_replay");
            if !ok {
                let msg = format!("LoadAndStep StepReplay: {:?}", err);
                error!("[inproc] {}", msg);
                let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
                    startup_id: Some(startup_id),
                    error: msg,
                }));
            } else {
                info!("[inproc] LoadAndStep complete (step ok)");
                inproc_poll_state(live_server, resp_tx);
            }
        }

        // ---------------------------------------------------------------
        // Strategy run — nautilus BacktestEngine path (issue #68 Slice 1)
        // ---------------------------------------------------------------
        TransportCommand::RunStrategy { strategy_file, config, startup_id } => {
            let strategy_file_str = strategy_file.to_string_lossy().to_string();

            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunStarted));
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::ReplayStartup {
                startup_id,
                stage: crate::trading::BackendStartupStage::StartingStrategy,
            }));

            // Build RustBacktestSink and call InprocLiveServer.start_nautilus_replay().
            // Slice 2: start_nautilus_replay() returns immediately after launching a
            // background thread; bars arrive via RustBacktestSink.push_bar() callbacks.
            // Completion is signalled by push_run_complete() or push_run_failed() from
            // the background thread.
            let result = {
                use pyo3::prelude::*;
                use pyo3::types::{PyDict, PyList};
                let resp_tx_clone = resp_tx.clone();
                Python::with_gil(|py| {
                    let sink = pyo3::Py::new(py, RustBacktestSink {
                        resp_tx: resp_tx_clone,
                        startup_id,
                    }).map_err(|e| format!("RustBacktestSink::new: {}", e))?;

                    let cfg = PyDict::new_bound(py);
                    cfg.set_item("strategy_file", &strategy_file_str)
                        .map_err(|e| e.to_string())?;
                    let py_ids = PyList::new_bound(py, config.instruments.iter().map(|s| s.as_str()));
                    cfg.set_item("instruments", py_ids)
                        .map_err(|e| e.to_string())?;
                    cfg.set_item("start_date", &config.start)
                        .map_err(|e| e.to_string())?;
                    cfg.set_item("end_date", &config.end)
                        .map_err(|e| e.to_string())?;
                    cfg.set_item("granularity", &config.granularity)
                        .map_err(|e| e.to_string())?;
                    cfg.set_item("rust_sink", sink.bind(py))
                        .map_err(|e| e.to_string())?;
                    if let Some(cp) = default_catalog.as_deref() {
                        cfg.set_item("catalog_path", cp)
                            .map_err(|e| e.to_string())?;
                    }
                    if let Some(ic) = config.initial_cash {
                        cfg.set_item("initial_cash", ic)
                            .map_err(|e| e.to_string())?;
                    }

                    let result = live_server
                        .bind(py)
                        .call_method1("start_nautilus_replay", (cfg,))
                        .map_err(|e| format!("start_nautilus_replay PyO3: {}", e))?;
                    inproc_json_dumps(py, result, "start_nautilus_replay")
                })
            };

            match result {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if success {
                        // Background thread started; completion arrives via
                        // push_run_complete() or push_run_failed() from Python.
                        info!("[inproc] RunStrategy start_nautilus_replay: background thread started");
                    } else {
                        // Validation failed synchronously (no strategy file, no catalog, etc.)
                        let msg = format!(
                            "start_nautilus_replay: {} {}",
                            r.get("error_code").and_then(|v| v.as_str()).unwrap_or(""),
                            r.get("error_message").and_then(|v| v.as_str()).unwrap_or(""),
                        );
                        error!("[inproc] RunStrategy {}", msg);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
                            startup_id: Some(startup_id), error: msg,
                        }));
                    }
                }
                Err(msg) => {
                    error!("[inproc] RunStrategy start_nautilus_replay error: {}", msg);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed {
                        startup_id: Some(startup_id), error: msg,
                    }));
                }
            }
        }

        // ---------------------------------------------------------------
        // Live commands — delegate to InprocLiveServer (Phase 4)
        // ---------------------------------------------------------------
        TransportCommand::SetExecutionMode { mode } => {
            match inproc_live_call(live_server, "set_execution_mode", |py, kwargs| {
                let _ = kwargs.set_item("mode", mode.as_wire_str());
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let em = r.get("execution_mode").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if success {
                        info!("[inproc] SetExecutionMode ok execution_mode={}", em);
                    } else {
                        error!("[inproc] SetExecutionMode rejected: error_code={} target={}", ec, mode.as_wire_str());
                    }
                }
                Err(msg) => error!("[inproc] SetExecutionMode error: {}", msg),
            }
        }
        TransportCommand::VenueLogin { venue_id, credentials_source, environment_hint } => {
            match inproc_live_call(live_server, "venue_login", |py, kwargs| {
                kwargs.set_item("venue_id", &venue_id)?;
                kwargs.set_item("credentials_source", &credentials_source)?;
                kwargs.set_item("environment_hint", &environment_hint)?;
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let vs = r.get("venue_state").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let il = r.get("instruments_loaded").and_then(|v| v.as_u64()).unwrap_or(0);
                    if success {
                        info!("[inproc] VenueLogin ok: venue_id={} venue_state={} instruments_loaded={}", venue_id, vs, il);
                        if let Some(parsed) = parse_venue_state(&vs) {
                            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::VenueChanged {
                                state: parsed,
                                venue_id: Some(venue_id),
                                instruments_loaded: il as u32,
                            }));
                        }
                    } else {
                        error!("[inproc] VenueLogin rejected: venue_id={} error_code={}", venue_id, ec);
                    }
                }
                Err(msg) => error!("[inproc] VenueLogin error: {}", msg),
            }
        }
        TransportCommand::VenueLogout => {
            match inproc_live_call(live_server, "venue_logout", |_py, _kwargs| Ok(())) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if success {
                        info!("[inproc] VenueLogout ok");
                    } else {
                        error!("[inproc] VenueLogout rejected: {:?}", r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] VenueLogout error: {}", msg),
            }
        }
        TransportCommand::ListInstruments { source } => {
            let source_str = tickers_source_to_wire(source);
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::InstrumentsListStarted { source }));
            match inproc_live_call(live_server, "list_instruments", |_py, kwargs| {
                let _ = kwargs.set_item("source", &source_str);
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if success {
                        let ids: Vec<String> = r.get("instrument_ids")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_owned())).collect())
                            .unwrap_or_default();
                        let instruments: Vec<crate::trading::Ticker> = r.get("instruments")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|x| {
                                Some(crate::trading::Ticker {
                                    id: x.get("id")?.as_str()?.to_owned(),
                                    name: x.get("name")?.as_str()?.to_owned(),
                                    market: x.get("market")?.as_str()?.to_owned(),
                                })
                            }).collect())
                            .unwrap_or_default();
                        info!("[inproc] ListInstruments ok: {} instruments", instruments.len());
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::InstrumentsListed { source, instruments }));
                    } else {
                        let err = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        warn!("[inproc] ListInstruments failed: {}", err);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::InstrumentsListFailed { source, error: err }));
                    }
                }
                Err(msg) => {
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::InstrumentsListFailed { source, error: msg }));
                }
            }
        }
        TransportCommand::FetchAvailableInstruments { end_date } => {
            let end_date_str = end_date.format("%Y-%m-%d").to_string();
            match inproc_live_call(live_server, "list_all_listed_symbols", |_py, kwargs| {
                let _ = kwargs.set_item("end_date", &end_date_str);
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if success {
                        let ids: Vec<String> = r.get("instrument_ids")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_owned())).collect())
                            .unwrap_or_default();
                        let resolved = r.get("resolved_end_date").and_then(|v| v.as_str()).unwrap_or(&end_date_str).to_string();
                        if resolved != end_date_str {
                            info!("[inproc] ListAllListedSymbols: backend clamped end_date {} -> {} ({} ids)", end_date_str, resolved, ids.len());
                        }
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::AvailableInstrumentsLoaded { end_date, ids }));
                    } else {
                        let err = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::AvailableInstrumentsFetchFailed { end_date, error: err }));
                    }
                }
                Err(msg) => {
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::AvailableInstrumentsFetchFailed { end_date, error: msg }));
                }
            }
        }
        TransportCommand::SubscribeMarketData { instrument_id } => {
            match inproc_live_call(live_server, "subscribe_market_data", |_py, kwargs| {
                let _ = kwargs.set_item("instrument_id", &instrument_id);
                Ok(())
            }) {
                Ok(r) => {
                    if r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        info!("[inproc] SubscribeMarketData ok: {}", instrument_id);
                    } else {
                        warn!("[inproc] SubscribeMarketData rejected: {} error_code={:?}", instrument_id, r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] SubscribeMarketData error: {} {}", instrument_id, msg),
            }
        }
        TransportCommand::UnsubscribeMarketData { instrument_id } => {
            match inproc_live_call(live_server, "unsubscribe_market_data", |_py, kwargs| {
                let _ = kwargs.set_item("instrument_id", &instrument_id);
                Ok(())
            }) {
                Ok(r) => {
                    if r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        info!("[inproc] UnsubscribeMarketData ok: {}", instrument_id);
                    } else {
                        warn!("[inproc] UnsubscribeMarketData rejected: {} error_code={:?}", instrument_id, r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] UnsubscribeMarketData error: {} {}", instrument_id, msg),
            }
        }
        TransportCommand::PlaceOrder { venue, instrument_id, side, qty, price, order_type, time_in_force, second_secret } => {
            match inproc_live_call(live_server, "place_order", |_py, kwargs| {
                let _ = kwargs.set_item("venue", &venue);
                let _ = kwargs.set_item("instrument_id", &instrument_id);
                let _ = kwargs.set_item("side", &side);
                let _ = kwargs.set_item("qty", qty);
                let _ = kwargs.set_item("price", price);
                let _ = kwargs.set_item("order_type", &order_type);
                let _ = kwargs.set_item("time_in_force", &time_in_force);
                let _ = kwargs.set_item("second_secret", second_secret.as_ref().map(|s| s.expose().to_string()));
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if success {
                        if let Some(ev) = r.get("order_event").filter(|v| !v.is_null()) {
                            let coid = ev.get("client_order_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let void = ev.get("venue_order_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let status = ev.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let filled_qty = ev.get("filled_qty").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let avg_price = ev.get("avg_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let ts_ms = ev.get("ts_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                            let strat_id = ev.get("strategy_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            info!("[inproc] PlaceOrder ok: {} {} {} qty={} status={} client_order_id={}", venue, side, instrument_id, qty, status, coid);
                            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderSeeded {
                                client_order_id: coid,
                                venue_order_id: void,
                                symbol: instrument_id,
                                side,
                                qty,
                                price,
                                status,
                                filled_qty,
                                avg_price,
                                ts_ms,
                                strategy_id: strat_id,
                            }));
                        } else {
                            warn!("[inproc] PlaceOrder ok but no order_event returned: {}", instrument_id);
                            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderNotice {
                                message: "発注応答が不完全です — venue で注文状態を確認してください".to_string(),
                            }));
                        }
                    } else {
                        warn!("[inproc] PlaceOrder rejected: {} error_code={}", instrument_id, ec);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderRejected {
                            action: "発注".to_string(),
                            error_code: ec,
                        }));
                    }
                }
                Err(msg) => {
                    error!("[inproc] PlaceOrder error: {} {}", instrument_id, msg);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderNotice {
                        message: "通信エラー — venue で注文状態を確認してください (発注)".to_string(),
                    }));
                }
            }
        }
        TransportCommand::CancelOrder { venue, order_id, second_secret } => {
            match inproc_live_call(live_server, "cancel_order", |_py, kwargs| {
                let _ = kwargs.set_item("venue", &venue);
                let _ = kwargs.set_item("order_id", &order_id);
                let _ = kwargs.set_item("second_secret", second_secret.as_ref().map(|s| s.expose().to_string()));
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if success {
                        if let Some(ev) = r.get("order_event").filter(|v| !v.is_null()) {
                            let coid = ev.get("client_order_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let void = ev.get("venue_order_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let status = ev.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let filled_qty = ev.get("filled_qty").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let avg_price = ev.get("avg_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let ts_ms = ev.get("ts_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                            info!("[inproc] CancelOrder ok: order_id={} status={}", order_id, status);
                            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderStatusUpdated {
                                client_order_id: coid,
                                venue_order_id: void,
                                status,
                                filled_qty,
                                avg_price,
                                ts_ms,
                            }));
                        }
                    } else {
                        warn!("[inproc] CancelOrder rejected: order_id={} error_code={}", order_id, ec);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderRejected {
                            action: "取消".to_string(),
                            error_code: ec,
                        }));
                    }
                }
                Err(msg) => {
                    error!("[inproc] CancelOrder error: {} {}", order_id, msg);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderNotice {
                        message: "通信エラー — venue で注文状態を確認してください (取消)".to_string(),
                    }));
                }
            }
        }
        TransportCommand::ModifyOrder { venue, client_order_id, new_qty, new_price, second_secret } => {
            match inproc_live_call(live_server, "modify_order", |_py, kwargs| {
                let _ = kwargs.set_item("venue", &venue);
                let _ = kwargs.set_item("client_order_id", &client_order_id);
                let _ = kwargs.set_item("new_qty", new_qty);
                let _ = kwargs.set_item("new_price", new_price);
                let _ = kwargs.set_item("second_secret", second_secret.as_ref().map(|s| s.expose().to_string()));
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if success {
                        if let Some(ev) = r.get("order_event").filter(|v| !v.is_null()) {
                            let coid = ev.get("client_order_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let void = ev.get("venue_order_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let status = ev.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let filled_qty = ev.get("filled_qty").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let avg_price = ev.get("avg_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let ts_ms = ev.get("ts_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                            info!("[inproc] ModifyOrder ok: client_order_id={} status={}", client_order_id, status);
                            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderModified {
                                client_order_id: coid,
                                venue_order_id: void,
                                new_qty,
                                new_price,
                                status,
                                filled_qty,
                                avg_price,
                                ts_ms,
                            }));
                        } else {
                            warn!("[inproc] ModifyOrder ok but no order_event: {}", client_order_id);
                            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderNotice {
                                message: "発注応答が不完全です — venue で注文状態を確認してください".to_string(),
                            }));
                        }
                    } else {
                        warn!("[inproc] ModifyOrder rejected: {} error_code={}", client_order_id, ec);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderRejected {
                            action: "訂正".to_string(),
                            error_code: ec,
                        }));
                    }
                }
                Err(msg) => {
                    error!("[inproc] ModifyOrder error: {} {}", client_order_id, msg);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::OrderNotice {
                        message: "通信エラー — venue で注文状態を確認してください (訂正)".to_string(),
                    }));
                }
            }
        }
        TransportCommand::GetOrders { venue } => {
            inproc_seed_orders(live_server, venue, resp_tx, false);
        }
        TransportCommand::GetOrdersAndReconcile { venue } => {
            inproc_seed_orders(live_server, venue, resp_tx, true);
        }
        TransportCommand::SubmitSecret { request_id, secret } => {
            match inproc_live_call(live_server, "submit_secret", |_py, kwargs| {
                let _ = kwargs.set_item("request_id", &request_id);
                let _ = kwargs.set_item("secret", secret.expose().to_string());
                Ok(())
            }) {
                Ok(r) => {
                    if r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        info!("[inproc] SubmitSecret ok: request_id={}", request_id);
                    } else {
                        let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        warn!("[inproc] SubmitSecret rejected: request_id={} error_code={}", request_id, ec);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::SecretSubmitFailed { error_code: ec }));
                    }
                }
                Err(msg) => error!("[inproc] SubmitSecret error: {} {}", request_id, msg),
            }
        }
        TransportCommand::ForceAccountSnapshot => {
            match inproc_live_call(live_server, "force_account_snapshot", |_py, _kwargs| Ok(())) {
                Ok(r) => {
                    if r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        info!("[inproc] ForceAccountSnapshot accepted; awaiting AccountEvent on stream");
                    } else {
                        error!("[inproc] ForceAccountSnapshot rejected: error_code={:?}", r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] ForceAccountSnapshot error: {}", msg),
            }
        }
        TransportCommand::StartLiveAuto { instrument_id, venue, strategy_file } => {
            let strategy_file_str = strategy_file.to_string_lossy().to_string();
            // Step 1: RegisterLiveStrategy
            let reg_result = inproc_live_call(live_server, "register_live_strategy", |_py, kwargs| {
                let _ = kwargs.set_item("strategy_file", &strategy_file_str);
                Ok(())
            });
            let strategy_id = match reg_result {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !success {
                        let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let em = r.get("error_message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let msg = crate::backend_sync::build_register_reject_message(
                            false, &ec, &em, &instrument_id, &venue,
                        ).unwrap_or_else(|| format!("RegisterLiveStrategy rejected: {}", ec));
                        error!("[inproc] {}", msg);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed { startup_id: None, error: msg }));
                        return;
                    }
                    r.get("strategy_id").and_then(|v| v.as_str()).unwrap_or("").to_string()
                }
                Err(msg) => {
                    let full = format!("RegisterLiveStrategy failed: instrument_id={} venue={} err={}", instrument_id, venue, msg);
                    error!("[inproc] {}", full);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed { startup_id: None, error: full }));
                    return;
                }
            };
            // Step 2: StartLiveStrategy
            let safety = default_live_auto_safety_limits(&instrument_id);
            match inproc_live_call(live_server, "start_live_strategy", |py, kwargs| {
                use pyo3::types::{PyDict, PyList};
                kwargs.set_item("strategy_id", &strategy_id)?;
                kwargs.set_item("instrument_id", &instrument_id)?;
                kwargs.set_item("venue", &venue)?;
                let sl = PyDict::new_bound(py);
                sl.set_item("max_position_size_jpy", safety.max_position_size_jpy)?;
                sl.set_item("max_order_value_jpy", safety.max_order_value_jpy)?;
                sl.set_item("max_daily_loss_jpy", safety.max_daily_loss_jpy)?;
                sl.set_item("max_orders_per_minute", safety.max_orders_per_minute)?;
                let instr = PyList::new_bound(py, safety.allowed_instruments.iter().map(|s| s.as_str()));
                sl.set_item("allowed_instruments", instr)?;
                kwargs.set_item("safety_limits_dict", sl)?;
                Ok(())
            }) {
                Ok(r) => {
                    let success = r.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !success {
                        let ec = r.get("error_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let em = r.get("error_message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let msg = crate::backend_sync::build_start_reject_message(
                            false, &ec, &em, &strategy_id, &instrument_id, &venue,
                        ).unwrap_or_else(|| format!("StartLiveStrategy rejected: {}", ec));
                        error!("[inproc] {}", msg);
                        let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed { startup_id: None, error: msg }));
                    }
                }
                Err(msg) => {
                    let full = format!("StartLiveStrategy failed: strategy_id={} instrument_id={} venue={} err={}", strategy_id, instrument_id, venue, msg);
                    error!("[inproc] {}", full);
                    let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::RunFailed { startup_id: None, error: full }));
                }
            }
        }
        TransportCommand::PauseLiveStrategy { run_id } => {
            match inproc_live_call(live_server, "pause_live_strategy", |_py, kwargs| {
                let _ = kwargs.set_item("run_id", &run_id);
                Ok(())
            }) {
                Ok(r) => {
                    if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        error!("[inproc] PauseLiveStrategy rejected: run_id={} error_code={:?}", run_id, r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] PauseLiveStrategy error: run_id={} err={}", run_id, msg),
            }
        }
        TransportCommand::ResumeLiveStrategy { run_id } => {
            match inproc_live_call(live_server, "resume_live_strategy", |_py, kwargs| {
                let _ = kwargs.set_item("run_id", &run_id);
                Ok(())
            }) {
                Ok(r) => {
                    if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        error!("[inproc] ResumeLiveStrategy rejected: run_id={} error_code={:?}", run_id, r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] ResumeLiveStrategy error: run_id={} err={}", run_id, msg),
            }
        }
        TransportCommand::StopLiveStrategy { run_id } => {
            match inproc_live_call(live_server, "stop_live_strategy", |_py, kwargs| {
                let _ = kwargs.set_item("run_id", &run_id);
                Ok(())
            }) {
                Ok(r) => {
                    if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        error!("[inproc] StopLiveStrategy rejected: run_id={} error_code={:?}", run_id, r.get("error_code"));
                    }
                }
                Err(msg) => error!("[inproc] StopLiveStrategy error: run_id={} err={}", run_id, msg),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        flush_stale_transport_commands, inproc_startup_status_sequence, map_backend_event_payload,
        parse_replay_granularity,
    };
    use crate::trading::TransportCommand;

    /// issue #64 finding #2 (RED): when InprocLiveServer init fails *after* DataEngine
    /// init succeeded, the worker must NOT report Connected(true)/Running(true). It must
    /// surface the failure as Connected(false) + Error so the UI does not show a healthy
    /// connection while every live command hits `None.method`.
    #[test]
    fn inproc_live_server_init_failure_reports_disconnect_not_connected() {
        use crate::trading::BackendStatusUpdate;
        let seq = super::inproc_startup_status_sequence(/*live_server_ok=*/ false);
        assert!(
            !seq.iter().any(|u| matches!(u, BackendStatusUpdate::Connected(true))),
            "must not report Connected(true) when live server init fails: {:?}",
            seq
        );
        assert!(
            seq.iter().any(|u| matches!(u, BackendStatusUpdate::Connected(false))),
            "must report Connected(false) on live server init failure: {:?}",
            seq
        );
        assert!(
            seq.iter().any(|u| matches!(u, BackendStatusUpdate::Error(_))),
            "must surface an Error on live server init failure: {:?}",
            seq
        );
    }

    /// issue #64 finding #7 (RED): the gRPC transport fires an initial
    /// ListInstruments(ReplayCatalogFallback) on connect so the instrument-dependent
    /// UI is seeded at startup. The InProc worker must perform the SAME startup fetch,
    /// otherwise the universe is empty only in in-proc mode (a regression).
    #[test]
    fn inproc_startup_fetches_instruments_like_grpc() {
        use crate::trading::TickersSource;
        assert_eq!(
            super::inproc_startup_instrument_fetch(),
            Some(TickersSource::ReplayCatalogFallback),
            "InProc startup must fetch instruments with the same source as gRPC \
             (fire_list_instruments at backend_transport.rs:185)"
        );
    }

    /// issue #64 finding #3 (RED): the InProc `inproc_poll_state` swallows a
    /// `get_state_json()` failure with `.ok()?`/`.ok()` — no log, no status.
    /// The gRPC GetState path always surfaces a failure (Error status or warn!,
    /// :1182-1196). InProc must NOT silently drop: a poll error must be logged
    /// before skipping.
    #[test]
    fn inproc_poll_state_failure_is_not_silent() {
        use super::PollStateOutcome;
        assert_eq!(
            super::inproc_poll_state_outcome(Err(())),
            PollStateOutcome::LogAndSkip,
            "a get_state_json() failure must be surfaced (warn!) before skipping, \
             not silently dropped — matching gRPC GetState (:1182-1196)"
        );
    }

    /// §3.8 regression: the reconnect flush must PRESERVE only
    /// `GetOrdersAndReconcile` / `FetchAvailableInstruments` and DROP everything else.
    #[test]
    fn reconnect_flush_preserves_only_get_orders_and_reconcile() {
        let drained = vec![
            TransportCommand::Pause,
            TransportCommand::GetOrdersAndReconcile {
                venue: "tachibana".to_string(),
            },
            TransportCommand::GetOrders {
                venue: "tachibana".to_string(),
            },
            TransportCommand::Resume,
            TransportCommand::CancelOrder {
                venue: "tachibana".to_string(),
                order_id: "co-1".to_string(),
                second_secret: None,
            },
            TransportCommand::StepForward,
            TransportCommand::ForceStop,
            TransportCommand::SubmitSecret {
                request_id: "r-old".to_string(),
                secret: crate::trading::RedactedSecret::new("hunter2".to_string()),
            },
            TransportCommand::VenueLogout,
        ];
        let preserved = flush_stale_transport_commands(drained);
        assert_eq!(
            preserved.len(),
            1,
            "only reconcile-class commands survive the flush"
        );
        assert!(
            matches!(preserved[0], TransportCommand::GetOrdersAndReconcile { ref venue } if venue == "tachibana"),
            "post-restart GetOrdersAndReconcile must survive the flush"
        );
        assert!(
            !preserved
                .iter()
                .any(|c| matches!(c, TransportCommand::GetOrders { .. })),
            "plain GetOrders must be dropped on the reconnect edge"
        );
        assert!(
            !preserved
                .iter()
                .any(|c| matches!(c, TransportCommand::CancelOrder { .. })),
            "stale order commands must be dropped"
        );
        assert!(
            !preserved
                .iter()
                .any(|c| matches!(c, TransportCommand::SubmitSecret { .. })),
            "stale SubmitSecret must be dropped"
        );
    }

    /// §4.6.2 / issue #53: FetchAvailableInstruments must survive the reconnect flush.
    #[test]
    fn reconnect_flush_preserves_fetch_available_instruments() {
        let end_date = chrono::NaiveDate::from_ymd_opt(2025, 5, 21).unwrap();
        let drained = vec![
            TransportCommand::Pause,
            TransportCommand::FetchAvailableInstruments { end_date },
            TransportCommand::GetOrdersAndReconcile {
                venue: "tachibana".to_string(),
            },
        ];
        let preserved = flush_stale_transport_commands(drained);
        assert!(
            preserved.iter().any(|c| {
                matches!(c, TransportCommand::FetchAvailableInstruments { end_date: d } if *d == end_date)
            }),
            "FetchAvailableInstruments must survive the reconnect flush"
        );
    }

    #[test]
    fn parse_replay_granularity_daily() {
        assert_eq!(
            parse_replay_granularity("Daily").unwrap(),
            crate::trading::engine::ReplayGranularity::Daily as i32
        );
    }

    #[test]
    fn parse_replay_granularity_minute() {
        assert_eq!(
            parse_replay_granularity("Minute").unwrap(),
            crate::trading::engine::ReplayGranularity::Minute as i32
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

    // ---------------------------------------------------------------------------
    // Phase 3: map_backend_event_payload unit tests
    // ---------------------------------------------------------------------------

    #[test]
    fn map_payload_secret_required() {
        use crate::trading::engine;
        let payload = engine::backend_event::Payload::SecretRequired(engine::SecretRequired {
            request_id: "req-1".into(),
            venue: "TACHIBANA".into(),
            kind: "password".into(),
            purpose: "second_auth".into(),
        });
        let ev = map_backend_event_payload(payload);
        assert!(
            matches!(ev, crate::trading::BackendEvent::SecretRequired { ref request_id, ref venue, .. }
                if request_id == "req-1" && venue == "TACHIBANA"),
            "SecretRequired payload should map correctly"
        );
    }

    #[test]
    fn map_payload_venue_logout_detected() {
        use crate::trading::engine;
        let payload = engine::backend_event::Payload::VenueLogoutDetected(
            engine::VenueLogoutDetected { venue: "KABU".into() },
        );
        let ev = map_backend_event_payload(payload);
        assert!(
            matches!(ev, crate::trading::BackendEvent::VenueLogoutDetected { ref venue } if venue == "KABU"),
            "VenueLogoutDetected payload should map correctly"
        );
    }

    #[test]
    fn map_payload_backend_error() {
        use crate::trading::engine;
        let payload = engine::backend_event::Payload::BackendError(engine::BackendError {
            source: "test".into(),
            detail: "something broke".into(),
            ts_ms: 9999,
        });
        let ev = map_backend_event_payload(payload);
        assert!(
            matches!(ev, crate::trading::BackendEvent::BackendError { ref source, ts_ms, .. }
                if source == "test" && ts_ms == 9999),
            "BackendError payload should map correctly"
        );
    }

    /// issue #64 finding #2 (RED): `GrpcTransport::run()` must build its future
    /// *without* eagerly calling `tokio::spawn` in the synchronous body. At startup
    /// `main.rs` evaluates `transport.run(...)` on the Bevy compute pool (no Tokio
    /// reactor) before handing the future to `handle.spawn`. An eager `tokio::spawn`
    /// there panics with "no reactor running". This test calls `run()` with NO
    /// runtime and only constructs the future (never polls it); it must not panic.
    #[test]
    fn grpc_run_does_not_spawn_eagerly_outside_runtime() {
        use super::BackendTransport;
        use crate::backend_supervisor::BackendLifecycle;
        use crate::trading::{
            BackendEvent, BackendStatusUpdate, BackendTradingState, TransportCommand,
        };
        use tokio::sync::mpsc;

        let transport = Box::new(super::GrpcTransport {
            url: "http://127.0.0.1:1".to_string(),
            token: "test-token".to_string(),
            poll_interval_ms: 1000,
            catalog_path: None,
        });

        let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel::<TransportCommand>();
        let (state_tx, _state_rx) = mpsc::unbounded_channel::<BackendTradingState>();
        let (status_tx, _status_rx) = mpsc::unbounded_channel::<BackendStatusUpdate>();
        let (event_tx, _event_rx) = mpsc::unbounded_channel::<BackendEvent>();
        let (_life_tx, life_rx) =
            tokio::sync::watch::channel(BackendLifecycle::NotStarted);

        // Constructing the future must not require a Tokio runtime. Currently the
        // events subscriber `tokio::spawn` lives in the synchronous body (~:90) and
        // panics here; after the fix it moves inside `Box::pin(async move { ... })`.
        let _fut = transport.run(cmd_rx, state_tx, status_tx, event_tx, life_rx);
        // We deliberately do NOT poll `_fut`; dropping it is enough for this guard.
    }
}
