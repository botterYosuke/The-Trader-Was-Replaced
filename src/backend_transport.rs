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

        Box::pin(async move {
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
/// GIL (cheap, in-memory), then release it with `py.allow_threads` for the
/// non-blocking channel send.
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
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::Connected(true)));
            let _ = resp_tx.send(InProcResp::Status(BackendStatusUpdate::Running(true)));
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

    let poll_duration = std::time::Duration::from_millis(poll_interval_ms);

    loop {
        // Wait for a command; on timeout, poll GetState.  GIL is NOT held here.
        let cmd = cmd_rx.recv_timeout(poll_duration);

        match cmd {
            Ok(cmd) => {
                inproc_dispatch(&engine, cmd, &resp_tx, &catalog_path);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                inproc_poll_state(&engine, &resp_tx);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                info!("[inproc] command channel closed; Python worker exiting");
                break;
            }
        }
    }
}

/// Call `engine.get_current_state().model_dump_json()` and forward the JSON.
fn inproc_poll_state(engine: &Py<PyAny>, resp_tx: &mpsc::UnboundedSender<InProcResp>) {
    use pyo3::prelude::*;

    let result = Python::with_gil(|py| -> Option<String> {
        let state = engine.bind(py).call_method0("get_current_state").ok()?;
        let json = state.call_method0("model_dump_json").ok()?;
        json.extract::<String>().ok()
    });

    if let Some(json) = result {
        let _ = resp_tx.send(InProcResp::StateJson(json));
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

/// Dispatch a single `TransportCommand` to the appropriate Python call.
fn inproc_dispatch(
    engine: &Py<PyAny>,
    cmd: TransportCommand,
    resp_tx: &mpsc::UnboundedSender<InProcResp>,
    default_catalog: &Option<String>,
) {
    match cmd {
        TransportCommand::Pause => {
            let (ok, err) = inproc_call_replay(engine, "pause_replay");
            if ok {
                info!("[inproc] PauseReplay ok");
            } else {
                error!("[inproc] PauseReplay failed: {:?}", err);
            }
        }
        TransportCommand::Resume => {
            let (ok, err) = inproc_call_replay(engine, "resume_replay");
            if ok {
                info!("[inproc] ResumeReplay ok");
            } else {
                error!("[inproc] ResumeReplay failed: {:?}", err);
            }
        }
        TransportCommand::StepForward => {
            let (ok, err) = inproc_call_replay(engine, "step_replay");
            if ok {
                info!("[inproc] StepReplay ok");
                // Immediately push updated state after a step.
                inproc_poll_state(engine, resp_tx);
            } else {
                error!("[inproc] StepReplay failed: {:?}", err);
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
                inproc_poll_state(engine, resp_tx);
            }
        }
        TransportCommand::RunStrategy { .. } => {
            warn!("[inproc] RunStrategy not supported in Phase 2 InProc; use GrpcTransport for full strategy runs");
        }
        other => {
            warn!("[inproc] command {:?} not supported in Phase 2 InProc (replay-only)", other);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{flush_stale_transport_commands, map_backend_event_payload, parse_replay_granularity};
    use crate::trading::TransportCommand;

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
}
