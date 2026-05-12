use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub mod engine {
    tonic::include_proto!("engine");
}

pub use engine::{StartRequest, StopRequest};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendTradingState {
    pub price: f32,
    pub history: Vec<f32>,
    pub timestamp: f64,
}

#[derive(Resource, Default)]
pub struct BackendStatus {
    pub connected: bool,
    pub running: bool,
    pub last_error: Option<String>,
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

impl TradingSettings {
    pub fn from_env() -> Self {
        Self {
            backend_enabled: std::env::var("BACKEND_ENABLED")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false),
            backend_url: std::env::var("BACKEND_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:19876".to_string()),
            token: std::env::var("BACKEND_TOKEN")
                .unwrap_or_else(|_| "dev-token".to_string()),
            poll_interval_ms: std::env::var("BACKEND_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500),
        }
    }
}

impl Default for TradingSettings {
    fn default() -> Self {
        Self::from_env()
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
    settings: Res<TradingSettings>,
    mut data: ResMut<TradingData>,
    mut channel: ResMut<BackendChannel>,
) {
    if !settings.backend_enabled {
        return;
    }

    while let Ok(state) = channel.rx.try_recv() {
        data.price = state.price;
        data.history = state.history;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_settings_from_env_defaults() {
        unsafe {
            std::env::remove_var("BACKEND_ENABLED");
            std::env::remove_var("BACKEND_URL");
        }
        let settings = TradingSettings::from_env();
        assert_eq!(settings.backend_enabled, false);
        assert_eq!(settings.backend_url, "http://127.0.0.1:19876");
    }

    #[test]
    #[serial]
    fn test_settings_from_env_custom() {
        unsafe {
            std::env::set_var("BACKEND_ENABLED", "true");
            std::env::set_var("BACKEND_URL", "http://localhost:1234");
        }
        let settings = TradingSettings::from_env();
        assert_eq!(settings.backend_enabled, true);
        assert_eq!(settings.backend_url, "http://localhost:1234");
    }

    #[test]
    fn test_backend_update_logic() {
        let mut world = World::new();
        let (tx, rx) = mpsc::unbounded_channel();
        
        world.insert_resource(TradingData::default());
        world.insert_resource(TradingSettings {
            backend_enabled: true,
            ..TradingSettings::from_env()
        });
        world.insert_resource(BackendChannel { rx });

        let new_state = BackendTradingState {
            price: 150.0,
            history: vec![140.0, 150.0],
            timestamp: 12345.67,
        };
        tx.send(new_state).unwrap();

        let mut schedule = Schedule::default();
        schedule.add_systems(backend_update_system);
        schedule.run(&mut world);

        let data = world.resource::<TradingData>();
        assert_eq!(data.price, 150.0);
        assert_eq!(data.history, vec![140.0, 150.0]);
    }

    #[test]
    fn test_backend_update_disabled_skips() {
        let mut world = World::new();
        let (tx, rx) = mpsc::unbounded_channel();
        
        world.insert_resource(TradingData::default());
        world.insert_resource(TradingSettings {
            backend_enabled: false,
            ..TradingSettings::from_env()
        });
        world.insert_resource(BackendChannel { rx });

        let new_state = BackendTradingState {
            price: 150.0,
            history: vec![140.0, 150.0],
            timestamp: 12345.67,
        };
        tx.send(new_state).unwrap();

        let mut schedule = Schedule::default();
        schedule.add_systems(backend_update_system);
        schedule.run(&mut world);

        let data = world.resource::<TradingData>();
        assert_ne!(data.price, 150.0); // Should not be updated
    }
}
