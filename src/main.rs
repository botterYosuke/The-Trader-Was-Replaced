mod trading;
mod ui;
mod grid;
mod camera;

use bevy::prelude::*;
use bevy_pancam::PanCamPlugin;
use bevy_vector_shapes::prelude::*;
use trading::{TradingData, price_simulation_system, backend_update_system, TradingSettings, BackendChannel, engine};
use ui::UiPlugin;
use grid::GridPlugin;
use camera::setup_camera;
use tokio::sync::mpsc;
use engine::data_engine_client::DataEngineClient;
use engine::GetStateRequest;

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
        .add_systems(Startup, (setup_camera, setup_backend_connection))
        .add_systems(Update, (
            price_simulation_system,
            backend_update_system,
        ))
        .run();
}

fn setup_backend_connection(mut commands: Commands, settings: Res<TradingSettings>) {
    let (tx, rx) = mpsc::unbounded_channel();
    commands.insert_resource(BackendChannel { rx });

    if !settings.backend_enabled {
        info!("Backend connection is disabled. Running in simulation mode.");
        return;
    }

    info!("Backend connection is enabled. Connecting to {}...", settings.backend_url);

    let url = settings.backend_url.clone();
    let token = settings.token.clone();
    let interval = settings.poll_interval_ms;

    tokio::spawn(async move {
        // Simple retry logic for initial connection
        let mut client = loop {
            match DataEngineClient::connect(url.clone()).await {
                Ok(c) => break c,
                Err(e) => {
                    eprintln!("Failed to connect to backend: {}. Retrying in 2s...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        };

        loop {
            let request = tonic::Request::new(GetStateRequest {
                token: token.clone(),
            });

            match client.get_state(request).await {
                Ok(response) => {
                    let json_data = response.into_inner().json_data;
                    if let Ok(state) = serde_json::from_str::<trading::BackendTradingState>(&json_data) {
                        let _ = tx.send(state);
                    }
                }
                Err(e) => {
                    eprintln!("gRPC error: {}. Attempting to reconnect...", e);
                    // Attempt to reconnect on error
                    if let Ok(c) = DataEngineClient::connect(url.clone()).await {
                        client = c;
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
        }
    });
}
