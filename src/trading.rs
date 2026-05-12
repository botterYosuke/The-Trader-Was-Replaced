use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub mod engine {
    tonic::include_proto!("engine");
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendTradingState {
    pub price: f32,
    pub history: Vec<f32>,
    pub timestamp: f64,
}

#[derive(Resource)]
pub struct TradingData {
    pub price: f32,
    pub history: Vec<f32>,
    pub timer: Timer,
}

impl Default for TradingData {
    fn default() -> Self {
        Self {
            price: 100.0,
            history: vec![100.0],
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
        }
    }
}

#[derive(Resource)]
pub struct TradingSettings {
    pub backend_enabled: bool,
    pub backend_url: String,
    pub token: String,
    pub poll_interval_ms: u64,
}

impl Default for TradingSettings {
    fn default() -> Self {
        Self {
            backend_enabled: false,
            backend_url: "http://127.0.0.1:50051".to_string(),
            token: "dev-token".to_string(),
            poll_interval_ms: 500,
        }
    }
}

#[derive(Resource)]
pub struct BackendChannel {
    pub rx: mpsc::UnboundedReceiver<BackendTradingState>,
}

pub fn price_simulation_system(
    time: Res<Time>,
    settings: Res<TradingSettings>,
    mut data: ResMut<TradingData>,
) {
    if settings.backend_enabled {
        return;
    }

    data.timer.tick(time.delta());
    if data.timer.just_finished() {
        let mut rng = rand::thread_rng();
        let change = rng.gen_range(-0.5..0.6);
        data.price += change;
        let price = data.price;
        data.history.push(price);
        if data.history.len() > 50 {
            data.history.remove(0);
        }
    }
}

pub fn backend_update_system(
    mut data: ResMut<TradingData>,
    mut channel: ResMut<BackendChannel>,
) {
    while let Ok(state) = channel.rx.try_recv() {
        data.price = state.price;
        data.history = state.history;
    }
}
