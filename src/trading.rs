use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod engine {
    tonic::include_proto!("engine");
}

pub use engine::{StartRequest, StopRequest};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryPoint {
    pub timestamp_ms: i64,
    pub price: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendTradingState {
    pub price: f32,
    pub history: Vec<f32>,
    pub timestamp: f64,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub history_points: Vec<HistoryPoint>,
    #[serde(default)]
    pub open: Option<f32>,
    #[serde(default)]
    pub high: Option<f32>,
    #[serde(default)]
    pub low: Option<f32>,
    #[serde(default)]
    pub close: Option<f32>,
    #[serde(default)]
    pub open_time_ms: Option<i64>,
    #[serde(default)]
    pub replay_state: Option<String>,
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
    pub timestamp_ms: i64,
    pub history_points: Vec<HistoryPoint>,
    pub timer: Timer,
    pub open: Option<f32>,
    pub high: Option<f32>,
    pub low: Option<f32>,
    pub close: Option<f32>,
    pub open_time_ms: Option<i64>,
    pub replay_state: Option<String>,
}

impl Default for TradingData {
    fn default() -> Self {
        Self {
            price: 100.0,
            history: vec![100.0],
            timestamp_ms: 0,
            history_points: Vec::new(),
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
            open: None,
            high: None,
            low: None,
            close: None,
            open_time_ms: None,
            replay_state: None,
        }
    }
}

#[derive(Resource)]
pub struct TradingSettings {
    pub backend_enabled: bool,
    pub backend_url: String,
    pub token: String,
    pub poll_interval_ms: u64,
    pub max_history_points: usize,
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
            max_history_points: std::env::var("MAX_HISTORY_POINTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
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

#[derive(Debug, Clone)]
pub enum TransportCommand {
    Pause,
    Resume,
    StepForward,
    StartEngine {
        strategy_file: std::path::PathBuf,
    },
}

#[derive(Resource)]
pub struct TransportCommandSender {
    pub tx: mpsc::UnboundedSender<TransportCommand>,
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
        
        // Phase 5 fix: Use real Unix ms for timestamp_ms
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
            
        data.timestamp_ms = now_ms;
        data.history_points.push(HistoryPoint {
            timestamp_ms: now_ms,
            price,
        });

        if data.history.len() > settings.max_history_points {
            data.history.remove(0);
        }
        if data.history_points.len() > settings.max_history_points {
            data.history_points.remove(0);
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

        // Phase 5: timestamp_ms と history_points の同期
        data.timestamp_ms = state.timestamp_ms.unwrap_or((state.timestamp * 1000.0) as i64);
        data.history_points = state.history_points;

        // Phase 6: OHLC
        data.open = state.open;
        data.high = state.high;
        data.low = state.low;
        data.close = state.close;
        data.open_time_ms = state.open_time_ms;

        // Phase 7: replay_state
        data.replay_state = state.replay_state;
        
        // もし history_points が空で history がある場合は補完
        if data.history_points.is_empty() && !data.history.is_empty() {
            let count = data.history.len();
            data.history_points = data.history.iter().enumerate().map(|(i, &p)| {
                HistoryPoint {
                    timestamp_ms: data.timestamp_ms - ((count - 1 - i) as i64 * 1000),
                    price: p,
                }
            }).collect();
        }

        // Defensive limit on Rust side
        if data.history.len() > settings.max_history_points {
            let start = data.history.len() - settings.max_history_points;
            data.history = data.history[start..].to_vec();
        }
        if data.history_points.len() > settings.max_history_points {
            let start = data.history_points.len() - settings.max_history_points;
            data.history_points = data.history_points[start..].to_vec();
        }
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
            timestamp_ms: Some(12345670),
            history_points: vec![
                HistoryPoint { timestamp_ms: 12344670, price: 140.0 },
                HistoryPoint { timestamp_ms: 12345670, price: 150.0 },
            ],
            open: Some(145.0),
            high: Some(155.0),
            low: Some(143.0),
            close: Some(150.0),
            open_time_ms: Some(12344000),
            replay_state: Some("RUNNING".to_string()),
        };
        tx.send(new_state).unwrap();

        let mut schedule = Schedule::default();
        schedule.add_systems(backend_update_system);
        schedule.run(&mut world);

        let data = world.resource::<TradingData>();
        assert_eq!(data.price, 150.0);
        assert_eq!(data.history, vec![140.0, 150.0]);
        assert_eq!(data.timestamp_ms, 12345670);
        assert_eq!(data.history_points.len(), 2);
        assert_eq!(data.open, Some(145.0));
        assert_eq!(data.high, Some(155.0));
        assert_eq!(data.low, Some(143.0));
        assert_eq!(data.close, Some(150.0));
        assert_eq!(data.open_time_ms, Some(12344000));
        assert_eq!(data.replay_state, Some("RUNNING".to_string()));
    }

    #[test]
    fn test_backend_update_fallback_history_points() {
        let mut world = World::new();
        let (tx, rx) = mpsc::unbounded_channel();
        
        world.insert_resource(TradingData::default());
        world.insert_resource(TradingSettings {
            backend_enabled: true,
            ..TradingSettings::from_env()
        });
        world.insert_resource(BackendChannel { rx });

        // history_points is missing in state (old backend)
        let new_state = BackendTradingState {
            price: 150.0,
            history: vec![140.0, 150.0],
            timestamp: 1600000000.0,
            timestamp_ms: None,
            history_points: vec![],
            open: None,
            high: None,
            low: None,
            close: None,
            open_time_ms: None,
            replay_state: None,
        };
        tx.send(new_state).unwrap();

        let mut schedule = Schedule::default();
        schedule.add_systems(backend_update_system);
        schedule.run(&mut world);

        let data = world.resource::<TradingData>();
        assert_eq!(data.price, 150.0);
        assert_eq!(data.timestamp_ms, 1600000000000); // Fallback from timestamp
        assert_eq!(data.history_points.len(), 2);
        assert_eq!(data.history_points[1].timestamp_ms, 1600000000000);
        assert_eq!(data.history_points[0].timestamp_ms, 1600000000000 - 1000);
    }

    #[test]
    fn test_backend_update_defensive_cap() {
        let mut world = World::new();
        let (tx, rx) = mpsc::unbounded_channel();
        
        world.insert_resource(TradingData::default());
        world.insert_resource(TradingSettings {
            backend_enabled: true,
            max_history_points: 3, // Very small cap
            ..TradingSettings::from_env()
        });
        world.insert_resource(BackendChannel { rx });

        let new_state = BackendTradingState {
            price: 5.0,
            history: vec![1.0, 2.0, 3.0, 4.0, 5.0],
            timestamp: 100.0,
            timestamp_ms: Some(100000),
            history_points: (0..5).map(|i| HistoryPoint { timestamp_ms: i * 1000, price: i as f32 }).collect(),
            open: None,
            high: None,
            low: None,
            close: None,
            open_time_ms: None,
            replay_state: None,
        };
        tx.send(new_state).unwrap();

        let mut schedule = Schedule::default();
        schedule.add_systems(backend_update_system);
        schedule.run(&mut world);

        let data = world.resource::<TradingData>();
        assert_eq!(data.history.len(), 3);
        assert_eq!(data.history, vec![3.0, 4.0, 5.0]);
        assert_eq!(data.history_points.len(), 3);
        assert_eq!(data.history_points.last().unwrap().price, 4.0); // Wait, history_points in state was 0..5, so last is 4.0
    }
}
