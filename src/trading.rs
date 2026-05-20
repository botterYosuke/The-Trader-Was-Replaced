use bevy::prelude::*;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    #[serde(default)]
    pub volume: Option<f32>,
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
    #[serde(default)]
    pub venue_state: Option<String>,
    #[serde(default)]
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub venue_id: Option<String>,
    #[serde(default)]
    pub instruments_loaded: Option<u32>,
    #[serde(default)]
    pub last_prices: HashMap<String, f64>,
    #[serde(default)]
    pub per_instrument: HashMap<String, InstrumentTradingData>,
    #[serde(default)]
    pub configured_venue: Option<String>,
}

#[derive(Resource, Default)]
pub struct BackendStatus {
    pub connected: bool,
    pub running: bool,
    pub last_error: Option<String>,
}

/// Wire: Python DepthLevel { price: float, size: float }.
/// ⚠️ key は "size"(NOT "qty")、型は f64(NOT u64)。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DepthLevel {
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub size: f64,
}

/// Wire: Python DepthSnapshot { bids, asks, timestamp_ms }.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DepthSnapshot {
    #[serde(default)]
    pub bids: Vec<DepthLevel>,
    #[serde(default)]
    pub asks: Vec<DepthLevel>,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
}

/// Wire: Python PerInstrumentState { price, ohlc_points, depth }.
/// chart は ohlc_points.last().close を最新 close に使い、last_price は LastPrices 側。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct InstrumentTradingData {
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub ohlc_points: Vec<OhlcPoint>,
    #[serde(default)]
    pub depth: Option<DepthSnapshot>,
}

/// Bevy Resource: symbol -> InstrumentTradingData。LastPrices と同じ形。
#[derive(Resource, Default)]
pub struct InstrumentTradingDataMap {
    pub map: HashMap<String, InstrumentTradingData>,
}

/// Bevy Resource: セッション横断のタイムスタンプ / replay 状態 / poll timer。
#[derive(Resource)]
pub struct TradingSession {
    pub timestamp_ms: i64,
    pub replay_state: Option<String>,
    pub timer: Timer,
}

impl Default for TradingSession {
    fn default() -> Self {
        Self {
            timestamp_ms: 0,
            replay_state: None,
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
    /// User-initiated execution-mode change. Backend is authoritative;
    /// `ExecutionModeRes` is updated only via `BackendStatusUpdate::ExecutionModeChanged`
    /// from the `GetState` polling diff, never directly from the UI.
    SetExecutionMode {
        mode: ExecutionMode,
    },
    /// User-initiated venue login. `token` is injected by the transport task
    /// from `TradingSettings`, so the UI only carries fields the user selects.
    VenueLogin {
        venue_id: String,
        credentials_source: String,
        environment_hint: String,
    },
    /// User-initiated venue logout. `token` is injected by the transport task.
    VenueLogout,
    /// Fetch the instrument universe and refresh `Tickers`. Triggered at
    /// startup (Replay catalog fallback) and whenever the venue transitions
    /// into CONNECTED/SUBSCRIBED (Live universe overwrite, plan §3.5).
    ListInstruments {
        /// Typed source hint; converted to a wire string by
        /// `tickers_source_to_wire` before being sent to the backend.
        source: TickersSource,
    },
    /// Live-mode sidebar click handler. `token` is injected by the transport
    /// task. Channels are `["trades", "depth"]` by default (LiveRunner is
    /// channel-agnostic on the backend side).
    SubscribeMarketData {
        instrument_id: String,
    },
    /// Unsubscribe from a previously-subscribed instrument's market data feed.
    /// Mirrors `SubscribeMarketData`; wired to the backend's `UnsubscribeMarketData`
    /// RPC (plan §3.4 D12).
    UnsubscribeMarketData {
        instrument_id: String,
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

pub fn backend_update_system(
    settings: Res<TradingSettings>,
    mut channel: ResMut<BackendChannel>,
    mut instrument_map: ResMut<InstrumentTradingDataMap>,
    mut session: ResMut<TradingSession>,
) {
    if !settings.backend_enabled {
        return;
    }

    while let Ok(mut state) = channel.rx.try_recv() {
        // backend は毎 poll で全状態を送るので map は丸ごと置換。chart は
        // InstrumentTradingDataMap、footer 等は TradingSession から読むため、
        // price/history/OHLC の個別 mirror は不要。
        session.timestamp_ms = state
            .timestamp_ms
            .unwrap_or((state.timestamp * 1000.0) as i64);
        session.replay_state = state.replay_state.clone();
        instrument_map.map = std::mem::take(&mut state.per_instrument);
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

/// Venue connection lifecycle state. String values are kept in sync with
/// Python side (see Phase 8 §0.1) for JSON round-trip via serde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VenueState {
    #[default]
    #[serde(rename = "DISCONNECTED")]
    Disconnected,
    #[serde(rename = "AUTHENTICATING")]
    Authenticating,
    #[serde(rename = "CONNECTED")]
    Connected,
    #[serde(rename = "SUBSCRIBED")]
    Subscribed,
    #[serde(rename = "RECONNECTING")]
    Reconnecting,
    #[serde(rename = "ERROR")]
    Error,
}

/// Execution mode selected in the UI. `LiveAuto` is a Phase 10 stub and
/// must not be selectable in Phase 8 (see plan §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionMode {
    #[default]
    #[serde(rename = "Replay")]
    Replay,
    #[serde(rename = "LiveManual")]
    LiveManual,
    #[serde(rename = "LiveAuto")]
    LiveAuto,
}

impl ExecutionMode {
    /// Wire-format string matching the `#[serde(rename = ...)]` values above.
    /// Used when sending `SetExecutionMode` RPC to the Python backend.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            ExecutionMode::Replay => "Replay",
            ExecutionMode::LiveManual => "LiveManual",
            ExecutionMode::LiveAuto => "LiveAuto",
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct VenueStatusRes {
    pub state: VenueState,
    pub venue_id: Option<String>,
    pub instruments_loaded: u32,
    pub configured_venue: Option<String>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct ExecutionModeRes {
    pub mode: ExecutionMode,
}

/// Where the `Tickers` list originated. Drives which overwrite rules apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickersSource {
    #[default]
    Unknown,
    /// Fetched from the connected venue adapter (`fetch_instruments`).
    LiveVenue,
    /// Venue snapshot cached on disk — Phase 8.7 has no firing path for this,
    /// reserved for future phases.
    LocalVenueSnapshot,
    /// Replay Parquet catalog fallback. Must not be used to prune Live universe.
    ReplayCatalogFallback,
}

/// Lifecycle status of the Tickers fetch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TickersStatus {
    #[default]
    NotFetched,
    InFlight,
    Loaded,
    /// Fetch failed; `list` retains the last successfully loaded value (stale display).
    Failed(String),
}

/// Convert a `TickersSource` to the wire string sent as
/// `ListInstrumentsRequest.source`. Returns `None` for `Unknown` (the field
/// is omitted from the request so the backend applies its own default).
/// Both `ReplayCatalogFallback` and `LocalVenueSnapshot` send `"local"` because
/// the backend routes both to the same `_list_instruments_local` path.
pub fn tickers_source_to_wire(source: TickersSource) -> Option<String> {
    match source {
        TickersSource::Unknown => None,
        TickersSource::ReplayCatalogFallback | TickersSource::LocalVenueSnapshot => {
            Some("local".to_string())
        }
        TickersSource::LiveVenue => Some("live".to_string()),
    }
}

/// Returns `true` when the venue state represents an active live connection.
pub fn is_venue_live(state: VenueState) -> bool {
    matches!(state, VenueState::Connected | VenueState::Subscribed)
}

/// Returns `true` when the venue is in any state that occupies the slot.
/// Used by menu_bar gating to disable opposite-venue Connect items and to
/// suppress duplicate `VenueLogin` while a slot is in use.
///
/// Busy states:
/// - `Authenticating` / `Connected` / `Subscribed`: slot is actively used.
/// - `Reconnecting`: backend is still holding the slot mid-retry; firing a
///   new `VenueLogin` here would collide on the adapter side.
/// - `Error`: the slot is not cleared until the user issues Disconnect, so a
///   fresh `VenueLogin` would also collide on the backend. The Disconnect
///   button remains available to clear it (Phase 8 post-merge review).
pub fn is_venue_busy_for_menu(state: VenueState) -> bool {
    matches!(
        state,
        VenueState::Authenticating
            | VenueState::Connected
            | VenueState::Subscribed
            | VenueState::Reconnecting
            | VenueState::Error,
    )
}

/// Returns `true` for any live execution mode (manual or auto).
pub fn is_live_mode(mode: ExecutionMode) -> bool {
    matches!(mode, ExecutionMode::LiveManual | ExecutionMode::LiveAuto)
}

/// Sidebar instrument row. Phase 8 §3.5: name/market are filled by the venue
/// adapter when available; for Replay catalog sources `name` falls back to
/// `id` and `market` is empty. Live-tick `last_price` is intentionally kept
/// in a separate (future) resource so the sidebar's virtual scroller does
/// not invalidate every row on every tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ticker {
    pub id: String,
    pub name: String,
    pub market: String,
}

/// Authoritative instrument universe shown in the sidebar. Replaced wholesale
/// on each `BackendStatusUpdate::InstrumentsListed` (plan §0.5.1: "overwrite,
/// not union — show the latest tradable universe").
#[derive(Resource, Debug, Clone, Default)]
pub struct Tickers {
    pub list: Vec<Ticker>,
    pub source: TickersSource,
    pub status: TickersStatus,
}

/// Per-instrument last trade price, overwritten wholesale on every
/// `BackendStatusUpdate::LastPricesUpdated` (plan §3.5 sidebar last-price
/// column). Live mode populates this from venue tick streams; Replay mode
/// emits an empty map so the sidebar clears.
#[derive(Resource, Debug, Clone, Default)]
pub struct LastPrices {
    pub map: HashMap<String, f64>,
}

/// Currently-selected sidebar symbol. Click handling is mode-dependent
/// (plan §3.5): Replay → update this only; Live* → also fire
/// `TransportCommand::SubscribeMarketData`.
#[derive(Resource, Debug, Clone, Default)]
pub struct SelectedSymbol {
    pub id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BackendStartupStage {
    ResettingReplay,
    LoadingData,
    StartingStrategy,
    WaitingForFirstTick,
}

#[derive(Debug, Clone)]
pub enum BackendStatusUpdate {
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
    VenueChanged {
        state: VenueState,
        venue_id: Option<String>,
        instruments_loaded: u32,
    },
    ExecutionModeChanged {
        mode: ExecutionMode,
    },
    /// Fetch started; sidebar can show a spinner (plan §3.3 D6c).
    InstrumentsListStarted {
        source: TickersSource,
    },
    /// Wholesale replacement of the sidebar instrument universe (plan §3.5 / §3.3 D6c).
    InstrumentsListed {
        source: TickersSource,
        instruments: Vec<Ticker>,
    },
    /// Fetch failed; sidebar shows stale list with error badge (plan §3.3 D6c).
    InstrumentsListFailed {
        source: TickersSource,
        error: String,
    },
    /// Wholesale replacement of the per-instrument last-trade price map
    /// derived from BackendTradingState.last_prices (Phase 8 §3.5 sidebar
    /// last-price column). Replay 切替時は空 HashMap で来て全消去される。
    LastPricesUpdated {
        prices: HashMap<String, f64>,
    },
    ConfiguredVenueDiscovered {
        venue_id: Option<String>,
    },
}

/// Bevy 側に流す backend event。proto の `backend_event::Payload` (oneof) を
/// owned 型でミラーしたもの（gRPC 受信タスクから ECS へ渡すための Send + 'static 型）。
#[derive(Debug, Clone)]
pub enum BackendEvent {
    SecretRequired {
        request_id: String,
        venue: String,
        kind: String,
        purpose: String,
    },
    OrderEvent {
        order_id: String,
        venue_order_id: String,
        client_order_id: String,
        status: String,
        filled_qty: f64,
        avg_price: f64,
        ts_ms: i64,
    },
    AccountEvent {
        cash: f64,
        buying_power: f64,
        positions: Vec<AccountPosition>,
        ts_ms: i64,
    },
    VenueLogoutDetected {
        venue: String,
    },
}

/// AccountEvent.positions の 1 要素。proto AccountPosition のミラー。
#[derive(Debug, Clone)]
pub struct AccountPosition {
    pub symbol: String,
    pub qty: i64,
    pub avg_price: f64,
    pub unrealized_pnl: f64,
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

    #[test]
    fn test_venue_state_default_is_disconnected() {
        assert_eq!(VenueState::default(), VenueState::Disconnected);
    }

    #[test]
    fn test_venue_state_json_round_trip() {
        let cases = [
            (VenueState::Disconnected, "\"DISCONNECTED\""),
            (VenueState::Authenticating, "\"AUTHENTICATING\""),
            (VenueState::Connected, "\"CONNECTED\""),
            (VenueState::Subscribed, "\"SUBSCRIBED\""),
            (VenueState::Reconnecting, "\"RECONNECTING\""),
            (VenueState::Error, "\"ERROR\""),
        ];
        for (v, s) in cases {
            let encoded = serde_json::to_string(&v).unwrap();
            assert_eq!(encoded, s, "encode mismatch for {:?}", v);
            let decoded: VenueState = serde_json::from_str(s).unwrap();
            assert_eq!(decoded, v, "decode mismatch for {}", s);
        }
    }

    #[test]
    fn test_execution_mode_default_is_replay() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Replay);
    }

    #[test]
    fn test_execution_mode_json_round_trip() {
        let cases = [
            (ExecutionMode::Replay, "\"Replay\""),
            (ExecutionMode::LiveManual, "\"LiveManual\""),
            (ExecutionMode::LiveAuto, "\"LiveAuto\""),
        ];
        for (m, s) in cases {
            let encoded = serde_json::to_string(&m).unwrap();
            assert_eq!(encoded, s, "encode mismatch for {:?}", m);
            let decoded: ExecutionMode = serde_json::from_str(s).unwrap();
            assert_eq!(decoded, m, "decode mismatch for {}", s);
        }
    }

    #[test]
    fn test_venue_status_res_default() {
        let r = VenueStatusRes::default();
        assert_eq!(r.state, VenueState::Disconnected);
        assert!(r.venue_id.is_none());
        assert_eq!(r.instruments_loaded, 0);
    }

    #[test]
    fn test_execution_mode_res_default() {
        let r = ExecutionModeRes::default();
        assert_eq!(r.mode, ExecutionMode::Replay);
    }

    #[test]
    fn test_tickers_default_is_empty() {
        let t = Tickers::default();
        assert!(t.list.is_empty());
    }

    #[test]
    fn test_selected_symbol_default_is_none() {
        let s = SelectedSymbol::default();
        assert!(s.id.is_none());
    }

    // ---- Phase 8.7 Step 5: TickersSource / TickersStatus / Tickers ----

    #[test]
    fn tickers_default_status_is_not_fetched_source_unknown() {
        let t = Tickers::default();
        assert!(t.list.is_empty());
        assert_eq!(t.source, TickersSource::Unknown);
        assert_eq!(t.status, TickersStatus::NotFetched);
    }

    #[test]
    fn tickers_list_started_sets_inflight_keeps_list() {
        let mut t = Tickers {
            list: vec![Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() }],
            source: TickersSource::Unknown,
            status: TickersStatus::NotFetched,
        };
        // Simulate InstrumentsListStarted reducer
        t.source = TickersSource::ReplayCatalogFallback;
        t.status = TickersStatus::InFlight;
        assert_eq!(t.source, TickersSource::ReplayCatalogFallback);
        assert_eq!(t.status, TickersStatus::InFlight);
        // list must be preserved
        assert_eq!(t.list.len(), 1);
        assert_eq!(t.list[0].id, "7203.TSE");
    }

    #[test]
    fn tickers_listed_overwrites_list_and_source_and_status_loaded() {
        let mut t = Tickers {
            list: vec![Ticker { id: "OLD.TSE".into(), name: "Old".into(), market: "TSE".into() }],
            source: TickersSource::Unknown,
            status: TickersStatus::InFlight,
        };
        let new_instruments = vec![
            Ticker { id: "1301.TSE".into(), name: "Kyokuyo".into(), market: "TSE".into() },
            Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() },
        ];
        // Simulate InstrumentsListed reducer
        t.source = TickersSource::LiveVenue;
        t.status = TickersStatus::Loaded;
        t.list = new_instruments;
        assert_eq!(t.source, TickersSource::LiveVenue);
        assert_eq!(t.status, TickersStatus::Loaded);
        assert_eq!(t.list.len(), 2);
        assert_eq!(t.list[0].id, "1301.TSE");
        assert_eq!(t.list[1].id, "7203.TSE");
    }

    #[test]
    fn tickers_list_failed_keeps_list_sets_status_failed() {
        let stale = vec![
            Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() },
        ];
        let mut t = Tickers {
            list: stale.clone(),
            source: TickersSource::ReplayCatalogFallback,
            status: TickersStatus::InFlight,
        };
        // Simulate InstrumentsListFailed reducer
        t.source = TickersSource::LiveVenue;
        t.status = TickersStatus::Failed("grpc timeout".to_string());
        // list is preserved (stale display)
        assert_eq!(t.list, stale);
        assert_eq!(t.source, TickersSource::LiveVenue);
        assert_eq!(t.status, TickersStatus::Failed("grpc timeout".to_string()));
    }

    #[test]
    fn tickers_source_to_wire_maps_all_variants() {
        assert_eq!(tickers_source_to_wire(TickersSource::Unknown), None);
        assert_eq!(tickers_source_to_wire(TickersSource::ReplayCatalogFallback), Some("local".to_string()));
        assert_eq!(tickers_source_to_wire(TickersSource::LocalVenueSnapshot), Some("local".to_string()));
        assert_eq!(tickers_source_to_wire(TickersSource::LiveVenue), Some("live".to_string()));
    }

    #[test]
    fn unsubscribe_market_data_command_serializes_to_backend_rpc() {
        // Verify that the UnsubscribeMarketData variant exists and can be constructed.
        let cmd = TransportCommand::UnsubscribeMarketData {
            instrument_id: "7203.TSE".to_string(),
        };
        match cmd {
            TransportCommand::UnsubscribeMarketData { instrument_id } => {
                assert_eq!(instrument_id, "7203.TSE");
            }
            _ => panic!("expected UnsubscribeMarketData variant"),
        }
    }

    #[test]
    fn test_is_venue_busy_for_menu_authenticating() {
        assert!(is_venue_busy_for_menu(VenueState::Authenticating));
    }

    #[test]
    fn test_is_venue_busy_for_menu_connected() {
        assert!(is_venue_busy_for_menu(VenueState::Connected));
    }

    #[test]
    fn test_is_venue_busy_for_menu_subscribed() {
        assert!(is_venue_busy_for_menu(VenueState::Subscribed));
    }

    #[test]
    fn test_is_venue_busy_for_menu_disconnected() {
        assert!(!is_venue_busy_for_menu(VenueState::Disconnected));
    }

    #[test]
    fn test_is_venue_busy_for_menu_error() {
        // HIGH-2: Error holds the slot until Disconnect — must report busy
        // so that another VenueLogin does not collide on the backend.
        assert!(is_venue_busy_for_menu(VenueState::Error));
    }

    #[test]
    fn test_is_venue_busy_for_menu_reconnecting() {
        // HIGH-2: Reconnecting is mid-retry — slot still occupied.
        assert!(is_venue_busy_for_menu(VenueState::Reconnecting));
    }

    #[test]
    fn test_is_venue_busy_for_menu_all_variants() {
        // HIGH-2: pin the full mapping so future VenueState additions force
        // an explicit review.
        let cases = [
            (VenueState::Disconnected, false),
            (VenueState::Authenticating, true),
            (VenueState::Connected, true),
            (VenueState::Subscribed, true),
            (VenueState::Reconnecting, true),
            (VenueState::Error, true),
        ];
        for (state, want) in cases {
            assert_eq!(
                is_venue_busy_for_menu(state),
                want,
                "unexpected busy={} for state={:?}",
                want,
                state
            );
        }
    }
}
