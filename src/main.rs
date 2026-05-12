use Backcast::trading::{TradingData, price_simulation_system, backend_update_system, TradingSettings, BackendChannel, engine, BackendStatus};
use Backcast::ui::UiPlugin;
use Backcast::grid::GridPlugin;
use Backcast::camera::setup_camera;
use bevy::prelude::*;
use bevy_pancam::PanCamPlugin;
use bevy_vector_shapes::prelude::*;
use tokio::sync::mpsc;
use engine::data_engine_client::DataEngineClient;
use engine::{GetStateRequest, StartRequest};

#[tokio::main]
async fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Trader Dashboard - Premium Infinite Canvas".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(PanCamPlugin)
        .add_plugins(Shape2dPlugin::default())
        .add_plugins(UiPlugin)
        .add_plugins(GridPlugin)
        .insert_resource(TradingData::default())
        .insert_resource(TradingSettings::default())
        .insert_resource(BackendStatus::default())
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
}

fn status_update_system(
    mut status: ResMut<BackendStatus>,
    mut channel: ResMut<StatusUpdateChannel>,
) {
    while let Ok(update) = channel.rx.try_recv() {
        match update {
            BackendStatusUpdate::Connected(c) => status.connected = c,
            BackendStatusUpdate::Running(r) => status.running = r,
            BackendStatusUpdate::Error(e) => {
                status.last_error = Some(e);
                status.connected = false;
            }
        }
    }
}

fn setup_backend_connection(mut commands: Commands, settings: Res<TradingSettings>) {
    let (tx, rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendChannel { rx });

    let (status_tx, status_rx) = mpsc::unbounded_channel();
    commands.insert_resource(StatusUpdateChannel { rx: status_rx });

    if !settings.backend_enabled {
        info!("Backend connection is disabled. Running in simulation mode.");
        return;
    }

    info!("Backend connection is enabled. Connecting to {}...", settings.backend_url);

    let url = settings.backend_url.clone();
    let token = settings.token.clone();
    let interval = settings.poll_interval_ms;

    tokio::spawn(async move {
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

        let start_request = tonic::Request::new(StartRequest {
            token: token.clone(),
        });
        match client.start(start_request).await {
            Ok(_) => {
                info!("Backend engine started successfully.");
                let _ = status_tx.send(BackendStatusUpdate::Running(true));
            }
            Err(e) => {
                error!("Failed to start backend engine: {}", e);
                let _ = status_tx.send(BackendStatusUpdate::Error(format!("Start failed: {}", e)));
            }
        }

        loop {
            let request = tonic::Request::new(GetStateRequest {
                token: token.clone(),
            });

            match tokio::time::timeout(tokio::time::Duration::from_secs(2), client.get_state(request)).await {
                Ok(Ok(response)) => {
                    let json_data = response.into_inner().json_data;
                    match serde_json::from_str::<Backcast::trading::BackendTradingState>(&json_data) {
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
