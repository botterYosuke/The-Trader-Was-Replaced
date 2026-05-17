use bevy::prelude::*;
use chrono::NaiveDate;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

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
pub struct OhlcPoint {
    pub timestamp_ms: i64,
    pub open_time_ms: i64,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub close: f32,
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
    pub ohlc_points: Vec<OhlcPoint>,
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
    pub ohlc_points: Vec<OhlcPoint>,
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
            ohlc_points: Vec::new(),
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
    /// Path to ParquetDataCatalog used by LoadReplayData. Derived from ARTIFACTS_PATH env var as `{ARTIFACTS_PATH}/jquants-catalog`.
    pub catalog_path: Option<String>,
}

impl TradingSettings {
    pub fn from_env() -> Self {
        Self {
            backend_enabled: std::env::var("BACKEND_ENABLED")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false),
            backend_url: std::env::var("BACKEND_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:19876".to_string()),
            token: std::env::var("BACKEND_TOKEN").unwrap_or_else(|_| "dev-token".to_string()),
            poll_interval_ms: std::env::var("BACKEND_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500),
            max_history_points: std::env::var("MAX_HISTORY_POINTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            catalog_path: {
                let base = std::env::var("ARTIFACTS_PATH").unwrap_or_else(|_| {
                    std::env::current_dir()
                        .unwrap_or_default()
                        .join("artifacts")
                        .to_string_lossy()
                        .to_string()
                });
                let p = std::path::Path::new(&base).join("jquants-catalog");
                Some(p.to_string_lossy().to_string())
            },
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

/// Scenario fields loaded from the strategy's `<strategy>.json` sidecar.
/// Kept in trading.rs to avoid ui → trading circular dependency.
#[derive(Debug, Clone, Default)]
pub struct StrategyRunConfig {
    pub instruments: Vec<String>,
    pub start: String,
    pub end: String,
    pub granularity: String,
    pub initial_cash: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum TransportCommand {
    Pause,
    Resume,
    StepForward,
    ForceStop,
    SetSpeed(u32),
    RunStrategy {
        strategy_file: std::path::PathBuf,
        config: StrategyRunConfig,
        /// UI transport 内だけの相関 ID。backend/proto には送らず、
        /// BackendStatusUpdate と照合して stale な status update が新しい
        /// startup window を閉じないようにするために使う。
        startup_id: u64,
    },
    FetchAvailableInstruments {
        end_date: NaiveDate,
    },
}

#[derive(Resource, Debug, Clone)]
pub struct ReplaySpeed {
    pub current: u32,
}

impl Default for ReplaySpeed {
    fn default() -> Self {
        Self { current: 1 }
    }
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
        data.timestamp_ms = state
            .timestamp_ms
            .unwrap_or((state.timestamp * 1000.0) as i64);
        data.history_points = state.history_points;

        // Phase 6: OHLC
        data.open = state.open;
        data.high = state.high;
        data.low = state.low;
        data.close = state.close;
        data.open_time_ms = state.open_time_ms;

        // Phase 7: replay_state
        data.replay_state = state.replay_state;

        // Phase 7: OHLC history for multi-candle chart
        data.ohlc_points = state.ohlc_points;

        // もし history_points が空で history がある場合は補完
        if data.history_points.is_empty() && !data.history.is_empty() {
            let count = data.history.len();
            data.history_points = data
                .history
                .iter()
                .enumerate()
                .map(|(i, &p)| HistoryPoint {
                    timestamp_ms: data.timestamp_ms - ((count - 1 - i) as i64 * 1000),
                    price: p,
                })
                .collect();
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
        if data.ohlc_points.len() > settings.max_history_points {
            let start = data.ohlc_points.len() - settings.max_history_points;
            data.ohlc_points = data.ohlc_points[start..].to_vec();
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub fills_count: i64,
    pub equity_points: i64,
    pub total_pnl: f64,
    pub status: String,
}

pub fn parse_summary_json(json: &str) -> Option<RunSummary> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            warn!("failed to parse summary_json: {}", e);
            return None;
        }
    };
    Some(RunSummary {
        fills_count: v["fills_count"].as_i64().unwrap_or(0),
        equity_points: v["equity_points"].as_i64().unwrap_or(0),
        total_pnl: v["total_pnl"].as_f64().unwrap_or(0.0),
        status: v["status"].as_str().unwrap_or("unknown").to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum RunState {
    #[default]
    Idle,
    Running,
    Completed,
    Failed {
        error: String,
    },
}

#[derive(Resource, Default, Debug, Clone)]
pub struct LastRunResult {
    pub run_id: Option<String>,
    pub summary_json: Option<String>,
    pub parsed_summary: Option<RunSummary>,
    pub state: RunState,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct AvailableInstruments {
    /// end_date キーで全上場銘柄リストを保持する UI セッション内ミラー。
    pub by_end_date: HashMap<NaiveDate, Vec<String>>,
    /// 同一 end_date への並行 fetch 防止。
    pub in_flight: HashSet<NaiveDate>,
    /// 最後の fetch 失敗。picker 内のエラー行表示に使用。
    pub last_error: Option<(NaiveDate, String)>,
}

/// `1301` → `1301.TSE`（既定 venue `TSE` 付与）。Phase 7.5a 現行挙動を pin。
/// 規則変更は Phase 8 universe 統合まで凍結（計画書 §0.5 Q1）。
pub fn code_to_instrument_id(code: &str) -> String {
    format!("{}.TSE", code)
}

/// `1301.TSE` → `1301`。venue suffix を剥がすだけ。
pub fn instrument_id_to_code(instrument_id: &str) -> String {
    instrument_id.split('.').next().unwrap_or("").to_string()
}

#[derive(Debug, Clone, Default)]
pub struct PortfolioPosition {
    pub symbol: String,
    pub qty: i64,
    pub avg_price: f64,
    pub unrealized_pnl: f64,
}

#[derive(Debug, Clone, Default)]
pub struct PortfolioOrder {
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub status: String,
    pub ts_ms: i64,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct PortfolioState {
    pub buying_power: f64,
    pub cash: f64,
    pub equity: f64,
    pub positions: Vec<PortfolioPosition>,
    pub orders: Vec<PortfolioOrder>,
    pub loaded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_parse_summary_json_valid() {
        let json = r#"{"fills_count":2,"equity_points":57,"total_pnl":-410010.0,"status":"ok"}"#;
        let s = parse_summary_json(json).unwrap();
        assert_eq!(s.fills_count, 2);
        assert_eq!(s.equity_points, 57);
        assert!((s.total_pnl - -410010.0).abs() < 1.0);
        assert_eq!(s.status, "ok");
    }

    #[test]
    fn test_parse_summary_json_invalid() {
        let result = parse_summary_json("not json at all");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_summary_json_missing_fields() {
        let json = r#"{"fills_count":5}"#;
        let s = parse_summary_json(json).unwrap();
        assert_eq!(s.fills_count, 5);
        assert_eq!(s.equity_points, 0);
        assert_eq!(s.total_pnl, 0.0);
        assert_eq!(s.status, "unknown");
    }

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
                HistoryPoint {
                    timestamp_ms: 12344670,
                    price: 140.0,
                },
                HistoryPoint {
                    timestamp_ms: 12345670,
                    price: 150.0,
                },
            ],
            ohlc_points: vec![],
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
            ohlc_points: vec![],
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
            history_points: (0..5)
                .map(|i| HistoryPoint {
                    timestamp_ms: i * 1000,
                    price: i as f32,
                })
                .collect(),
            ohlc_points: vec![],
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

    #[test]
    fn test_code_to_instrument_id_round_trip_4_digit() {
        let id = code_to_instrument_id("1301");
        assert_eq!(id, "1301.TSE");
        assert_eq!(instrument_id_to_code(&id), "1301");
    }

    #[test]
    fn test_code_to_instrument_id_round_trip_5_digit() {
        let id = code_to_instrument_id("13010");
        assert_eq!(id, "13010.TSE");
        assert_eq!(instrument_id_to_code(&id), "13010");
    }

    #[test]
    fn test_available_instruments_replaces_old_instrument_list() {
        let av = AvailableInstruments::default();
        assert!(av.by_end_date.is_empty());
        assert!(av.in_flight.is_empty());
        assert!(av.last_error.is_none());
    }

    #[test]
    fn test_available_instruments_shape_does_not_reintroduce_old_or_bidirectional_state() {
        use std::collections::{HashMap, HashSet};
        let _av = AvailableInstruments {
            by_end_date: HashMap::new(),
            in_flight: HashSet::new(),
            last_error: None,
        };
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }
    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, val) };
            Self { key, prev }
        }
        fn unset(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    #[serial]
    fn test_catalog_path_uses_artifacts_path_env() {
        let _a = EnvGuard::set("ARTIFACTS_PATH", "/tmp/custom-artifacts");
        let _b = EnvGuard::unset("BACKEND_CATALOG_PATH");
        let settings = TradingSettings::from_env();
        let catalog = settings.catalog_path.expect("catalog_path should be Some");
        assert!(
            catalog.ends_with("jquants-catalog"),
            "expected jquants-catalog suffix, got: {catalog}"
        );
        assert!(
            catalog.contains("custom-artifacts"),
            "expected custom-artifacts in path, got: {catalog}"
        );
    }

    #[test]
    #[serial]
    fn test_catalog_path_defaults_to_artifacts_jquants_catalog() {
        let _a = EnvGuard::unset("ARTIFACTS_PATH");
        let _b = EnvGuard::unset("BACKEND_CATALOG_PATH");
        let settings = TradingSettings::from_env();
        let catalog = settings.catalog_path.expect("catalog_path should be Some");
        assert!(
            catalog.ends_with("jquants-catalog"),
            "expected jquants-catalog suffix, got: {catalog}"
        );
        assert!(
            catalog.contains("artifacts"),
            "expected 'artifacts' in path, got: {catalog}"
        );
    }

    #[test]
    #[serial]
    fn test_catalog_path_absolute_artifacts_path_not_joined_with_repo() {
        let _a = EnvGuard::set("ARTIFACTS_PATH", "/absolute/path");
        let _b = EnvGuard::unset("BACKEND_CATALOG_PATH");
        let settings = TradingSettings::from_env();
        let catalog = settings.catalog_path.expect("catalog_path should be Some");
        assert!(
            catalog.contains("absolute") && catalog.contains("path"),
            "expected /absolute/path to be base, got: {catalog}"
        );
        assert!(
            catalog.ends_with("jquants-catalog"),
            "expected jquants-catalog suffix, got: {catalog}"
        );
    }

    #[test]
    #[serial]
    fn test_catalog_path_ignores_backend_catalog_path_env() {
        let _a = EnvGuard::unset("ARTIFACTS_PATH");
        let _b = EnvGuard::set("BACKEND_CATALOG_PATH", "/legacy/path");
        let settings = TradingSettings::from_env();
        let catalog = settings.catalog_path.expect("catalog_path should be Some");
        assert!(
            !catalog.starts_with("/legacy/path"),
            "catalog must not use BACKEND_CATALOG_PATH, got: {catalog}"
        );
        assert!(
            catalog.ends_with("jquants-catalog"),
            "expected jquants-catalog suffix, got: {catalog}"
        );
    }
}
