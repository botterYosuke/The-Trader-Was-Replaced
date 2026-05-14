use backcast::trading::{
    backend_update_system, engine, price_simulation_system, BackendChannel, BackendStatus,
    LastRunResult, TradingData, TradingSettings, TransportCommand, TransportCommandSender,
};
use backcast::ui::UiPlugin;
use backcast::grid::GridPlugin;
use backcast::camera::setup_camera;
use bevy::prelude::*;
use bevy_pancam::PanCamPlugin;
use tokio::sync::mpsc;
use engine::data_engine_client::DataEngineClient;
use engine::{
    EngineStartConfig, EngineKind, GetStateRequest, LoadReplayDataRequest, PauseReplayRequest,
    ReplayGranularity, ResumeReplayRequest, StartEngineRequest, StepReplayRequest,
    StartEngineResponse,
};

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
        .add_plugins(PanCamPlugin)
        .add_plugins(UiPlugin)
        .add_plugins(GridPlugin)
        .insert_resource(TradingData::default())
        .insert_resource(TradingSettings::default())
        .insert_resource(BackendStatus::default())
        .insert_resource(LastRunResult::default())
        .insert_resource(tokio_handle)
        .add_systems(Startup, (setup_camera, setup_backend_connection))
        .add_systems(Update, (
            price_simulation_system,
            backend_update_system,
            status_update_system,
        ))
        .run();
}

#[derive(Resource)]
struct StatusUpdateChannel {
    rx: mpsc::UnboundedReceiver<BackendStatusUpdate>,
}

enum BackendStatusUpdate {
    Connected(bool),
    Running(bool),
    Error(String),
    RunComplete { run_id: String, summary_json: String },
}


fn status_update_system(
    mut status: ResMut<BackendStatus>,
    mut channel: ResMut<StatusUpdateChannel>,
    mut last_run: ResMut<LastRunResult>,
) {
    while let Ok(update) = channel.rx.try_recv() {
        match update {
            BackendStatusUpdate::Connected(c) => status.connected = c,
            BackendStatusUpdate::Running(r) => status.running = r,
            BackendStatusUpdate::Error(e) => {
                status.last_error = Some(e);
                status.connected = false;
            }
            BackendStatusUpdate::RunComplete { run_id, summary_json } => {
                info!("RunComplete: run_id={} summary={}", run_id, summary_json);
                last_run.run_id = Some(run_id);
                last_run.summary_json = Some(summary_json);
            }
        }
    }
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

    info!("Backend connection is enabled. Connecting to {}...", settings.backend_url);

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
                    TransportCommand::RunStrategy { strategy_file, config } => {
                        let strategy_file_str = strategy_file.to_string_lossy().to_string();

                        // Map granularity string → proto enum
                        let granularity_i32 = match config.granularity.as_str() {
                            "Daily"  => Some(ReplayGranularity::Daily as i32),
                            "Minute" => Some(ReplayGranularity::Minute as i32),
                            other => {
                                error!("RunStrategy: unknown granularity {:?}, aborting", other);
                                continue;
                            }
                        };

                        info!(
                            "RunStrategy: step1 LoadReplayData instruments={:?} start={:?} end={:?} granularity={:?} catalog_path={:?}",
                            config.instruments, config.start, config.end, config.granularity, catalog_path
                        );

                        // Step 1: LoadReplayData (IDLE → LOADED)
                        let load_req = tonic::Request::new(LoadReplayDataRequest {
                            request_id: String::new(),
                            instrument_ids: config.instruments.clone(),
                            start_date: config.start.clone(),
                            end_date: config.end.clone(),
                            granularity: granularity_i32,
                            token: token.clone(),
                            catalog_path: catalog_path.clone(),
                        });

                        match client.load_replay_data(load_req).await {
                            Ok(r) => {
                                let inner = r.into_inner();
                                if !inner.success {
                                    error!(
                                        "LoadReplayData rejected: code={}, msg={}",
                                        inner.error_code, inner.error_message
                                    );
                                    continue; // do not proceed to StartEngine
                                }
                                info!("LoadReplayData ok, state={:?}", inner.current_state);
                            }
                            Err(e) => {
                                error!("LoadReplayData gRPC error: {}", e);
                                continue;
                            }
                        }

                        // Step 2: StartEngine (LOADED → RUNNING)
                        info!("RunStrategy: step2 StartEngine strategy_file={:?}", strategy_file_str);
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
                            token: token.clone(),
                        });
                        match client.start_engine(start_req).await {
                            Ok(r) => {
                                let inner: StartEngineResponse = r.into_inner();
                                if inner.success {
                                    info!("StartEngine ok, state={:?}", inner.current_state);
                                    if let (Some(rid), Some(sj)) = (inner.run_id.as_deref(), inner.summary_json.as_deref()) {
                                        let _ = status_tx.send(BackendStatusUpdate::RunComplete {
                                            run_id: rid.to_owned(),
                                            summary_json: sj.to_owned(),
                                        });
                                    }
                                } else {
                                    error!(
                                        "StartEngine rejected: code={}, msg={}",
                                        inner.error_code.as_deref().unwrap_or(""),
                                        inner.error_message.as_deref().unwrap_or(""),
                                    );
                                }
                            }
                            Err(e) => error!("StartEngine gRPC error: {}", e),
                        }
                    }
                }
            }

            let request = tonic::Request::new(GetStateRequest {
                token: token.clone(),
            });

            match tokio::time::timeout(tokio::time::Duration::from_secs(2), client.get_state(request)).await {
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
                    let err_msg = "gRPC request timed out".to_string();
                    error!("{}. Attempting to reconnect...", err_msg);
                    let _ = status_tx.send(BackendStatusUpdate::Error(err_msg));
                    if let Ok(c) = DataEngineClient::connect(url.clone()).await {
                        client = c;
                        let _ = status_tx.send(BackendStatusUpdate::Connected(true));
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
        }
    });
}
