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

/// Phase 10 §2.4 / §0.6: the safety-rail limits the user sets in the
/// Promote-to-Live modal, carried in `TransportCommand::PromoteToLive` and mapped
/// to the proto `SafetyLimits` by the transport task. **`0` disables that rail**
/// (mirrors the backend `SafetyRails`, where `0 = rail off`). `allowed_instruments`
/// is the pre-trade whitelist; the modal defaults it to the single promoted
/// instrument. Transport-neutral mirror so the lib (UI) never imports the
/// generated proto types.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SafetyLimitsInput {
    pub max_position_size_jpy: i64,
    pub max_order_value_jpy: i64,
    pub max_daily_loss_jpy: i64,
    pub max_orders_per_minute: i32,
    pub allowed_instruments: Vec<String>,
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
    /// Phase 9 §3.2: manual new order. `token` is injected by the transport task.
    /// `client_order_id` is generated by the backend handler and returned in the
    /// response, so it is not carried here. `second_secret` is Tachibana-only
    /// (collected by the SecretModal); mock/kabu ignore it.
    PlaceOrder {
        venue: String,
        instrument_id: String,
        side: String,
        qty: f64,
        price: Option<f64>,
        order_type: String,
        time_in_force: String,
        second_secret: Option<RedactedSecret>,
    },
    /// Phase 9 §3.2: cancel an order by its `client_order_id`. `token` is injected
    /// by the transport task. `second_secret` is Tachibana-only (Step 5).
    CancelOrder {
        venue: String,
        order_id: String,
        second_secret: Option<RedactedSecret>,
    },
    /// Phase 9 §3.2 (Step 4): modify (訂正) an order by its `client_order_id`.
    /// `token` is injected by the transport task. `new_qty`/`new_price` are
    /// `None` when unchanged (proto `optional`). `second_secret` is Tachibana-only
    /// (Step 5; Step 4 always sends `None`). `OrderEvent` carries no qty/price, so
    /// the transport task merges these command-side values back into the
    /// `OrderModified` status update.
    ModifyOrder {
        venue: String,
        client_order_id: String,
        new_qty: Option<f64>,
        new_price: Option<f64>,
        second_secret: Option<RedactedSecret>,
    },
    /// Phase 9 §0.3: UI response to a `SecretRequired` event. `token` is injected
    /// by the transport task. The secret is forwarded to the backend and dropped;
    /// it is never stored in a resource or echoed to logs (§1.3 ADR).
    SubmitSecret {
        request_id: String,
        secret: RedactedSecret,
    },
    /// Phase 9 §3.8: list the backend's currently-tracked working orders to
    /// reconcile against the UI's optimistic `LiveOrders` after an auto-restart.
    /// `token` is injected by the transport task. The response drives
    /// `OrdersReconciled`.
    GetOrders {
        venue: String,
    },
    /// Phase 10 §2.7 / §1.3: promote the editor's saved strategy to a Live Auto
    /// run. The transport task expands this into the RPC chain
    /// `RegisterLiveStrategy` → (`SetExecutionMode(LiveAuto)` when `ensure_live_auto`)
    /// → `StartLiveStrategy`, **awaited in order** so the backend's
    /// `ExecutionMode == LiveAuto` precondition is satisfied before Start (firing
    /// the three as independent commands would race). `token` is injected by the
    /// transport task. `expected_sha256` is the TOCTOU guard (empty → backend
    /// computes only). The unary outcome comes back as
    /// `BackendStatusUpdate::LiveStrategyPromoteResult`; success ALSO arrives as a
    /// pushed `LiveStrategyEvent{status:"RUNNING"}`.
    PromoteToLive {
        strategy_file: std::path::PathBuf,
        expected_sha256: String,
        instrument_id: String,
        venue: String,
        params: HashMap<String, String>,
        safety_limits: SafetyLimitsInput,
        ensure_live_auto: bool,
    },
    /// Phase 10 §2.8: Live Run Panel controls. `token` is injected by the transport
    /// task. `Pause`/`Resume`/`Stop` are gated on `run_id` existence on the backend
    /// only (no mode hard-gate, §2.5) so a runaway run can always be stopped.
    PauseLiveStrategy {
        run_id: String,
    },
    ResumeLiveStrategy {
        run_id: String,
    },
    StopLiveStrategy {
        run_id: String,
    },
}

/// Wrapper around a Tachibana second password that redacts itself in `Debug`
/// output and zeroizes its backing memory on drop. Phase 9 §1.3 ADR: the
/// plaintext must never reach logs, files, or a long-lived state resource.
/// `TransportCommand` derives `Debug`, so a bare `String` here would risk the
/// secret appearing in any `{:?}` of a command — this newtype closes that hole.
///
/// The inner field is **private** on purpose: `expose()` is the single audited
/// read path (grep-able exfiltration point). A `pub` field would let any caller
/// re-derive the plaintext (`secret.0.clone()`, `format!("{}", secret.0)`),
/// defeating both the `Debug` redaction and the zero-on-drop guarantee.
#[derive(Clone)]
pub struct RedactedSecret(zeroize::Zeroizing<String>);

impl RedactedSecret {
    pub fn new(s: String) -> Self {
        Self(zeroize::Zeroizing::new(s))
    }

    /// Borrow the plaintext for the single moment it is copied into a gRPC
    /// request. Callers must not retain the returned `&str`.
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for RedactedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedactedSecret(***)")
    }
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

/// Execution mode selected in the UI. All three variants are user-selectable:
/// the footer renders a segment for each (`footer.rs`), `LiveAuto` drives the
/// promote-to-live strategy chain (`main.rs`), and `is_live_mode` treats both
/// `LiveManual` and `LiveAuto` as live.
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
    /// Phase 9 §3.2: full order record assembled by the transport task from a
    /// `PlaceOrder` response. The response's `OrderEvent` carries only ids +
    /// status + fills, so the transport task merges in the originating command's
    /// symbol/side/qty/price (which `OrderEvent` lacks) before sending this.
    /// Seeds the full record in `LiveOrders`.
    OrderSeeded {
        client_order_id: String,
        venue_order_id: String,
        symbol: String,
        side: String,
        qty: f64,
        price: Option<f64>,
        status: String,
        filled_qty: f64,
        avg_price: f64,
        ts_ms: i64,
        /// Phase 10 (§2.9 / M6): the ordering subject's Nautilus StrategyId. A
        /// manual `PlaceOrder` carries "MANUAL-001"; a non-empty value seeds the
        /// `LiveOrders` row so a later empty EC-stream `OrderEvent` cannot wipe it.
        strategy_id: String,
    },
    /// Phase 9 §3.2: status/fill update for an already-known order (e.g. a
    /// `CancelOrder` response, whose `OrderEvent` has no symbol/side/qty/price).
    /// Merged into the existing `LiveOrders` record by `client_order_id`.
    OrderStatusUpdated {
        client_order_id: String,
        venue_order_id: String,
        status: String,
        filled_qty: f64,
        avg_price: f64,
        ts_ms: i64,
    },
    /// Phase 9 §3.2 (Step 4): a `ModifyOrder` RPC succeeded. The response's
    /// `OrderEvent` carries ids + status + fills but **not** the new qty/price,
    /// so the transport task merges in the originating command's `new_qty`/
    /// `new_price` (Some => overwrite the tracked value, None => keep). Applied to
    /// the existing `LiveOrders` record by `client_order_id` (unknown id is a
    /// no-op — modify always targets a known order).
    OrderModified {
        client_order_id: String,
        venue_order_id: String,
        new_qty: Option<f64>,
        new_price: Option<f64>,
        status: String,
        filled_qty: f64,
        avg_price: f64,
        ts_ms: i64,
    },
    /// Phase 9 §2.2 / §3.9: a `PlaceOrder` / `CancelOrder` RPC was rejected
    /// (structured `success=false`, e.g. `EXECUTION_MODE_PRECONDITION` /
    /// `VENUE_LOGIN_REQUIRED` / venue error code). Surfaced to the user via
    /// `OrderFeedback` (OrderPanel error line) instead of being warn-only.
    OrderRejected {
        action: String,
        error_code: String,
    },
    /// Phase 9 §3.10: a `SubmitSecret` RPC was rejected (`success=false`). This is
    /// a SECRET-flow failure, not an order rejection — it is reduced into
    /// `SecretPrompt.error` (surfaced by the SecretModal) so the user can retry,
    /// NOT into `OrderFeedback` (which would pop the OrderPanel out of context and
    /// could be cleared by unrelated order updates).
    SecretSubmitFailed {
        error_code: String,
    },
    /// Phase 9 §3.10 / §2.2: a user-visible order-flow notice surfaced verbatim in
    /// the OrderPanel feedback line. Used for cases that are NOT a structured venue
    /// reject but still demand the trader's attention: an incomplete success
    /// response (`success=true` but no `order_event` → an accepted order we cannot
    /// track) and an order-RPC transport error (place/cancel/modify `Err(_)` →
    /// pressed-the-button-nothing-shown ambiguity). This IS an order-flow event, so
    /// the OrderFeedback channel is the correct bucket (unlike SecretSubmitFailed).
    OrderNotice {
        message: String,
    },
    /// Phase 9 §3.8: result of a post-restart `GetOrders` reconcile. Carries the
    /// `client_order_id`s the backend still tracks as working; `apply_status_update`
    /// diffs these against the UI's optimistic `LiveOrders` and populates
    /// `ReconcilePrompt` with the orders whose state is now unknown.
    OrdersReconciled {
        backend_client_order_ids: Vec<String>,
    },
    /// Phase 10 §2.7: structured outcome of a `PromoteToLive` RPC chain. Success
    /// also arrives as a pushed `LiveStrategyEvent{status:"RUNNING"}`, so this is
    /// primarily for surfacing a structured reject (`EXECUTION_MODE_PRECONDITION` /
    /// `VENUE_LOGIN_REQUIRED` / `LIVE_STRATEGY_ALREADY_RUNNING` /
    /// `STRATEGY_LOAD_FAILED` / `STRATEGY_HASH_MISMATCH`) to the user via
    /// `PromoteFeedback`. On success `run_id` is the new Live run.
    LiveStrategyPromoteResult {
        success: bool,
        error_code: String,
        run_id: String,
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
        /// Phase 10 (§2.9 / M6): the ordering subject's Nautilus StrategyId.
        /// "" until Step 7 populates it (manual → "MANUAL-001", auto → "LIVE-{run}").
        strategy_id: String,
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
    /// Phase 10 Step 3 (M8): a Live Auto run changed lifecycle state.
    LiveStrategyEvent {
        run_id: String,
        strategy_id: String,
        status: String,
        ts_ms: i64,
    },
    /// Phase 10 Step 3 (M8): a pre/post-trade safety rail blocked or tripped (§2.4).
    SafetyRailViolation {
        run_id: String,
        kind: String,
        detail: String,
        ts_ms: i64,
    },
    /// Phase 10 Step 3 (M8): relayed strategy `self.log.*` line for the Live Run Panel.
    StrategyLogMessage {
        run_id: String,
        level: String,
        message: String,
        ts_ms: i64,
    },
    /// Phase 10 Step 7 (§2.8 / §2.9): run-scoped PnL / order / fill counters,
    /// pushed periodically for the Live Run Panel telemetry cells. Separate from
    /// the lifecycle `LiveStrategyEvent` so it can arrive at any cadence and even
    /// before the first lifecycle event.
    LiveStrategyTelemetry {
        run_id: String,
        strategy_id: String,
        realized_pnl: f64,
        unrealized_pnl: f64,
        order_count: i64,
        fill_count: i64,
        ts_ms: i64,
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

/// A single Live-mode order tracked by the UI. Phase 9 §3.2: proto `OrderEvent`
/// carries only ids + status + fills, **not** symbol/side/qty/price. Those
/// static attributes are known only from the originating `PlaceOrder` request,
/// so the UI seeds them from `BackendStatusUpdate::OrderSeeded` (the unary
/// `PlaceOrder` response correlated with the command) and then merges
/// status/fill updates from `BackendEvent::OrderEvent` / `OrderStatusUpdated`
/// by `client_order_id`.
#[derive(Debug, Clone, Default)]
pub struct LiveOrder {
    pub client_order_id: String,
    pub venue_order_id: String,
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: Option<f64>,
    pub status: String,
    pub filled_qty: f64,
    pub avg_price: f64,
    pub ts_ms: i64,
    /// Phase 10 (§2.9 / M6): ordering subject's StrategyId for the OrdersPanel
    /// filter. Populated in Step 7; "" in Step 3 (additive mirror only).
    pub strategy_id: String,
}

impl LiveOrder {
    /// Advance the cumulative fill MONOTONICALLY (§3.12). A cancel/modify RPC
    /// response carries `filled_qty=0.0` (or only the new leg's fill); it must NOT
    /// clobber a larger partial already recorded by the EC stream, or a real-money
    /// position is under-reported. Fills never legitimately decrease (incl. kabu
    /// telescoping, which keeps the wire value cumulative). Shared by `apply_event`
    /// and `apply_modify` so this real-money invariant stays identical.
    fn advance_fill(&mut self, filled_qty: f64, avg_price: f64) {
        if filled_qty >= self.filled_qty {
            self.filled_qty = filled_qty;
            self.avg_price = avg_price;
        }
    }
}

/// Live-mode order book as seen by the UI, keyed by `client_order_id`.
/// Populated by the order RPC response + `OrderEvent` push (Step 3). The Replay
/// path keeps using `PortfolioState.orders`; this resource is read by the
/// OrdersPanel only in Live execution modes.
#[derive(Resource, Default, Debug, Clone)]
pub struct LiveOrders {
    pub orders: Vec<LiveOrder>,
}

/// Retained Live order count. The panel shows only the newest 6; the rest is
/// headroom for recent cancels/fills. A Live session is append-only (each new
/// `client_order_id` inserts), so cap retention to bound memory and the
/// per-frame work in the OrdersPanel.
const MAX_LIVE_ORDERS: usize = 64;

impl LiveOrders {
    /// Seed or replace the full record for `client_order_id` (used by the
    /// `PlaceOrder` response, which is the only source of symbol/side/qty/price).
    pub fn upsert_full(&mut self, order: LiveOrder) {
        if let Some(existing) = self
            .orders
            .iter_mut()
            .find(|o| o.client_order_id == order.client_order_id)
        {
            *existing = order;
        } else {
            self.orders.insert(0, order);
            self.orders.truncate(MAX_LIVE_ORDERS);
        }
    }

    /// Merge a status/fill update from an `OrderEvent`. Static fields
    /// (symbol/side/qty/price) are preserved when the order is already known;
    /// an unknown `client_order_id` is inserted with empty static fields so the
    /// event is still visible (e.g. orders placed before this session).
    ///
    /// Phase 10 (§2.9 / M6) `strategy_id` merge invariant: a **non-empty**
    /// `strategy_id` overwrites the stored one; an **empty** value never clears a
    /// known one. This lets a unary `PlaceOrder` response (MANUAL-001) or the auto
    /// bridge (LIVE-..) tag a row that a later untagged EC-stream `OrderEvent`
    /// (strategy_id="") must not wipe — same pattern as `venue_order_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_event(
        &mut self,
        client_order_id: &str,
        venue_order_id: &str,
        status: &str,
        filled_qty: f64,
        avg_price: f64,
        ts_ms: i64,
        strategy_id: &str,
    ) {
        if let Some(existing) = self
            .orders
            .iter_mut()
            .find(|o| o.client_order_id == client_order_id)
        {
            if !venue_order_id.is_empty() {
                existing.venue_order_id = venue_order_id.to_string();
            }
            if !strategy_id.is_empty() {
                existing.strategy_id = strategy_id.to_string();
            }
            // status / venue_order_id / ts_ms always refresh; cumulative fill is
            // advanced monotonically (see LiveOrder::advance_fill, §3.12).
            existing.status = status.to_string();
            existing.advance_fill(filled_qty, avg_price);
            existing.ts_ms = ts_ms;
        } else {
            self.orders.insert(
                0,
                LiveOrder {
                    client_order_id: client_order_id.to_string(),
                    venue_order_id: venue_order_id.to_string(),
                    status: status.to_string(),
                    filled_qty,
                    avg_price,
                    ts_ms,
                    strategy_id: strategy_id.to_string(),
                    ..Default::default()
                },
            );
            self.orders.truncate(MAX_LIVE_ORDERS);
        }
    }

    /// Orders matching `filter`, newest-first (the storage order), as borrowed
    /// refs (§2.9). Both the OrdersPanel cell renderer and its right-click hit
    /// observer index into THIS view so the displayed row N maps to the same order
    /// in both — filtering must never desync the two.
    pub fn filtered<'a>(&'a self, filter: &OrdersFilter) -> Vec<&'a LiveOrder> {
        self.orders
            .iter()
            .filter(|o| order_matches_filter(&o.strategy_id, filter))
            .collect()
    }

    /// The `n`-th order matching `filter` in storage order (newest-first), without
    /// allocating. The OrdersPanel pulls each displayed row (≤6/frame) this way and
    /// its right-click hit observer uses the same lookup, so row N maps to the same
    /// order in both — without the per-frame `Vec` that `filtered()` would build.
    pub fn nth_filtered<'a>(&'a self, filter: &OrdersFilter, n: usize) -> Option<&'a LiveOrder> {
        self.orders
            .iter()
            .filter(|o| order_matches_filter(&o.strategy_id, filter))
            .nth(n)
    }

    /// Distinct non-empty `strategy_id`s present in the book, in first-seen
    /// (newest-first) order (§2.9). Drives the filter's cycle options.
    pub fn distinct_strategy_ids(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for o in &self.orders {
            if !o.strategy_id.is_empty() && !seen.contains(&o.strategy_id) {
                seen.push(o.strategy_id.clone());
            }
        }
        seen
    }

    /// Merge a `ModifyOrder` (訂正) result into the existing record. `symbol`/
    /// `side` are preserved; `new_qty`/`new_price` overwrite `qty`/`price` only
    /// when `Some` (None => unchanged, matching the proto `optional` semantics);
    /// status/filled_qty/avg_price/venue_order_id/ts_ms are refreshed from the
    /// event. An unknown `client_order_id` is a **no-op** — a modify is only ever
    /// issued against a known order (Phase 9 §3.2 / §3.12).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_modify(
        &mut self,
        client_order_id: &str,
        venue_order_id: &str,
        new_qty: Option<f64>,
        new_price: Option<f64>,
        status: &str,
        filled_qty: f64,
        avg_price: f64,
        ts_ms: i64,
    ) {
        if let Some(existing) = self
            .orders
            .iter_mut()
            .find(|o| o.client_order_id == client_order_id)
        {
            if !venue_order_id.is_empty() {
                existing.venue_order_id = venue_order_id.to_string();
            }
            if let Some(q) = new_qty {
                existing.qty = q;
            }
            if let Some(p) = new_price {
                existing.price = Some(p);
            }
            existing.status = status.to_string();
            // Cumulative fill advanced monotonically (see LiveOrder::advance_fill,
            // §3.12): a CLMKabuCorrectOrder / kabu-remap ACCEPTED response may report
            // only the new leg's fill while the EC stream recorded a larger partial.
            existing.advance_fill(filled_qty, avg_price);
            existing.ts_ms = ts_ms;
        }
        // Unknown id: no-op (modify always targets a known order).
    }
}

/// Phase 10 §2.9: OrdersPanel strategy_id filter. The panel cycles through
/// `All` → `Manual` → each distinct strategy → `All`. Read by `orders_panel_system`
/// in Live mode only; the Replay path (`PortfolioState.orders`) ignores it.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub enum OrdersFilter {
    /// Show every order regardless of strategy_id.
    #[default]
    All,
    /// Manual orders only (`strategy_id == "MANUAL-001"`).
    Manual,
    /// A specific automated strategy (`strategy_id == "LIVE-…"`).
    Strategy(String),
}

/// The Nautilus StrategyId tag a manual `PlaceOrder` carries (§2.9). Centralised
/// here so the OrdersPanel filter and any producer-mirror use the same literal.
pub const MANUAL_STRATEGY_ID: &str = "MANUAL-001";

/// Whether an order with `order_strategy_id` is shown under `filter` (§2.9 pure
/// predicate, unit-tested). `All` always matches; `Manual` matches MANUAL-001;
/// `Strategy(s)` matches an exact strategy_id.
pub fn order_matches_filter(order_strategy_id: &str, filter: &OrdersFilter) -> bool {
    match filter {
        OrdersFilter::All => true,
        OrdersFilter::Manual => order_strategy_id == MANUAL_STRATEGY_ID,
        OrdersFilter::Strategy(s) => order_strategy_id == s,
    }
}

/// The ordered cycle of filter options for the current order book (§2.9):
/// `All`, then `Manual` (only if any MANUAL-001 order exists), then one
/// `Strategy(id)` per distinct non-manual strategy_id (newest-first). The cell
/// click cycles through this list and wraps to `All`.
pub fn filter_cycle(orders: &LiveOrders) -> Vec<OrdersFilter> {
    let mut cycle = vec![OrdersFilter::All];
    let distinct = orders.distinct_strategy_ids();
    if distinct.iter().any(|s| s == MANUAL_STRATEGY_ID) {
        cycle.push(OrdersFilter::Manual);
    }
    for s in distinct {
        if s != MANUAL_STRATEGY_ID {
            cycle.push(OrdersFilter::Strategy(s));
        }
    }
    cycle
}

/// Next filter in the cycle for `orders`, wrapping `All` → … → `All`. If `current`
/// is no longer present in the cycle (its strategy_id vanished), fall back to the
/// next option after `All` (or `All` if that's all there is).
pub fn next_filter(current: &OrdersFilter, orders: &LiveOrders) -> OrdersFilter {
    let cycle = filter_cycle(orders);
    match cycle.iter().position(|f| f == current) {
        Some(i) => cycle[(i + 1) % cycle.len()].clone(),
        // current dropped out of the cycle (e.g. its strategy retired): advance to
        // the next meaningful option, else stay at All.
        None => cycle.get(1).cloned().unwrap_or(OrdersFilter::All),
    }
}

/// Short display label for a strategy_id in the filter cell (§2.9):
/// "MANUAL-001" → "Manual", "LIVE-abcd…" → "Strategy: <tail>", else the raw id.
/// `n` is the tail length passed to the shared `short_id` shortener.
pub fn filter_label(filter: &OrdersFilter) -> String {
    match filter {
        OrdersFilter::All => "All".to_string(),
        OrdersFilter::Manual => "Manual".to_string(),
        OrdersFilter::Strategy(s) => format!("Strategy: {}", short_id(s, 8)),
    }
}

/// Tail-`n`-char shortener for ids (shared by the Live Run Panel and the OrdersPanel
/// filter so the two stay visually consistent). Short ids pass through unchanged;
/// longer ones are rendered as `…<last n chars>`.
pub fn short_id(id: &str, n: usize) -> String {
    let count = id.chars().count();
    if count <= n {
        return id.to_string();
    }
    let tail: String = id.chars().skip(count - n).collect();
    format!("…{tail}")
}

/// Phase 10 §2.8: one Live Auto run tracked by the UI for the Live Run Panel.
/// Populated from `BackendEvent::LiveStrategyEvent` pushes. Run-level PnL / order
/// / fill telemetry is NOT here yet — that needs a telemetry event (Step 7 / §2.9);
/// Step 6 shows lifecycle status + timing only.
#[derive(Debug, Clone, Default)]
pub struct LiveRunRecord {
    pub run_id: String,
    pub strategy_id: String,
    /// READY / RUNNING / PAUSED / STOPPING / STOPPED / ERROR.
    pub status: String,
    /// ts_ms of the first event seen for this run (run start).
    pub started_ts_ms: i64,
    /// ts_ms of the most recent event.
    pub updated_ts_ms: i64,
    /// Phase 10 Step 7 (§2.8 / §2.9): run-scoped telemetry from
    /// `LiveStrategyTelemetry` pushes. Lifecycle events (`apply_event`) never
    /// touch these; they default to 0 until the first telemetry push.
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub order_count: i64,
    pub fill_count: i64,
}

/// Terminal run states — settled, so no longer controllable.
pub fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "STOPPED" | "ERROR")
}

/// Live Auto runs as seen by the UI, newest first. Keyed by `run_id`. Read by the
/// Live Run Panel (§2.8). Phase 10 caps active automated runs to 1, but the list
/// retains recent terminal runs so the user sees their final state.
#[derive(Resource, Default, Debug, Clone)]
pub struct LiveRuns {
    pub runs: Vec<LiveRunRecord>,
}

const MAX_LIVE_RUNS: usize = 8;

impl LiveRuns {
    /// Upsert a run from a `LiveStrategyEvent`. `started_ts_ms` is fixed at the
    /// first event; later events only refresh `status` / `updated_ts_ms`. An empty
    /// `strategy_id` (older producer) never clears a known one.
    pub fn apply_event(&mut self, run_id: &str, strategy_id: &str, status: &str, ts_ms: i64) {
        if let Some(r) = self.runs.iter_mut().find(|r| r.run_id == run_id) {
            r.status = status.to_string();
            if !strategy_id.is_empty() {
                r.strategy_id = strategy_id.to_string();
            }
            r.updated_ts_ms = ts_ms;
        } else {
            self.runs.insert(
                0,
                LiveRunRecord {
                    run_id: run_id.to_string(),
                    strategy_id: strategy_id.to_string(),
                    status: status.to_string(),
                    started_ts_ms: ts_ms,
                    updated_ts_ms: ts_ms,
                    ..Default::default()
                },
            );
            self.runs.truncate(MAX_LIVE_RUNS);
        }
    }

    /// Merge a run-scoped telemetry push (§2.8 / §2.9) into the run's counters.
    /// Upserts the run so telemetry that races ahead of the first lifecycle event
    /// still creates a row (status is left empty until a `LiveStrategyEvent`
    /// arrives). The merge invariants mirror `apply_event`: a non-empty
    /// `strategy_id` wins / an empty one never clears a known one, and
    /// `started_ts_ms` is fixed at whichever event is seen first. Lifecycle status
    /// is never touched here (telemetry carries no status).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_telemetry(
        &mut self,
        run_id: &str,
        strategy_id: &str,
        realized_pnl: f64,
        unrealized_pnl: f64,
        order_count: i64,
        fill_count: i64,
        ts_ms: i64,
    ) {
        if let Some(r) = self.runs.iter_mut().find(|r| r.run_id == run_id) {
            if !strategy_id.is_empty() {
                r.strategy_id = strategy_id.to_string();
            }
            r.realized_pnl = realized_pnl;
            r.unrealized_pnl = unrealized_pnl;
            r.order_count = order_count;
            r.fill_count = fill_count;
            r.updated_ts_ms = ts_ms;
        } else {
            self.runs.insert(
                0,
                LiveRunRecord {
                    run_id: run_id.to_string(),
                    strategy_id: strategy_id.to_string(),
                    status: String::new(),
                    started_ts_ms: ts_ms,
                    updated_ts_ms: ts_ms,
                    realized_pnl,
                    unrealized_pnl,
                    order_count,
                    fill_count,
                },
            );
            self.runs.truncate(MAX_LIVE_RUNS);
        }
    }
}

/// Active `SecretRequired` prompt driving the SecretModal. Phase 9 §3.10:
/// Tachibana-only. The drain system sets `active` when a `SecretRequired` event
/// arrives; the modal opens while `active` is `Some` and clears it on
/// submit / cancel / timeout. The plaintext secret never lives here.
#[derive(Resource, Default, Debug, Clone)]
pub struct SecretPrompt {
    pub active: Option<SecretPromptRequest>,
    /// Phase 9 §3.10: error_code from a failed `SubmitSecret` RPC, surfaced by the
    /// SecretModal so the user can retry. This is NOT an order rejection — a secret
    /// failure must not pop the OrderPanel feedback line (wrong bucket; could be
    /// cleared by unrelated order updates). Cleared on submit / cancel / new prompt.
    pub error: Option<String>,
}

impl SecretPrompt {
    /// Fully close the prompt: drop the active request AND any stale submit error.
    /// Single choke point so no closing path can forget the `error` field and leave
    /// a stale rejection lingering for the next prompt (§3.10).
    pub fn close(&mut self) {
        self.active = None;
        self.error = None;
    }
}

#[derive(Debug, Clone)]
pub struct SecretPromptRequest {
    pub request_id: String,
    pub venue: String,
    pub kind: String,
    pub purpose: String,
}

/// Active venue-logout notice driving the ReloginModal (Phase 9 §3.5 / Step 7).
/// `backend_event_drain_system` sets `active` to the venue id when a
/// `VenueLogoutDetected` event arrives (kabu 本体早朝ログアウト / Tachibana 閉局). The
/// modal opens while `active` is `Some`, telling the user the venue dropped and to
/// re-login via the Venue menu. It clears on user dismiss.
///
/// **設計判断 (drift note)**: モーダルは「通知」に徹し、自身は `VenueLogin` を発射しない。
/// 検知時点で backend の `venue_sm` はまだ `CONNECTED`（検知は push であって状態遷移では
/// ない）なので、ここから直接 `VenueLogin` を撃つと busy slot に衝突する。実際の再ログインは
/// 既存の Venue メニュー (Disconnect→Connect) を通す——そちらが slot のクリアと環境
/// (demo/verify/prod) 選択を正しく所有している。誤った環境への再接続・二重発注リスクを避ける。
#[derive(Resource, Default, Debug, Clone)]
pub struct ReloginPrompt {
    pub active: Option<String>,
}

/// Latest user-facing notice for the manual-order flow (§3.10 / §2.2). Phase 9
/// has no toast/ModalLayer infrastructure yet (Phase 8 left venue-RPC rejects
/// warn-only), so order/secret failures that the user must see — RPC rejects
/// (`EXECUTION_MODE_PRECONDITION`, `VENUE_LOGIN_REQUIRED`, venue error codes)
/// and `SECRET_INPUT_CANCELED` on secret timeout — are surfaced in the
/// OrderPanel's existing error line via this resource until a proper toast
/// system lands (tracked for a later step).
#[derive(Resource, Default, Debug, Clone)]
pub struct OrderFeedback {
    pub message: Option<String>,
}

/// Phase 10 §2.7: latest user-facing notice for the Promote-to-Live flow. Set by
/// `apply_status_update` from a `LiveStrategyPromoteResult` (success → run id,
/// reject → error code) and surfaced by the Safety Rails modal / Promote button.
/// Distinct from `OrderFeedback` because the OrderPanel error line only shows in
/// `LiveManual`, whereas a promote outcome must be visible in `LiveAuto`.
#[derive(Resource, Default, Debug, Clone)]
pub struct PromoteFeedback {
    pub message: Option<String>,
}

/// Phase 10 §2.10: one safety-rail violation surfaced as a transient Footer toast.
/// `backend_event_drain_system` sets `active` from a `SafetyRailViolation` push; the
/// `safety_toast` UI system renders it and auto-expires it. This is the project's
/// first toast — `OrderFeedback`/`PromoteFeedback` are persistent inline lines, not
/// time-bounded overlays, so a violation needs its own channel (criterion line 484).
#[derive(Resource, Default, Debug, Clone)]
pub struct SafetyToast {
    pub active: Option<SafetyToastEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SafetyToastEntry {
    pub run_id: String,
    pub kind: String,
    pub detail: String,
    pub ts_ms: i64,
}

impl SafetyToast {
    /// Replace the active toast (a newer violation supersedes an older one).
    pub fn show(&mut self, run_id: String, kind: String, detail: String, ts_ms: i64) {
        self.active = Some(SafetyToastEntry {
            run_id,
            kind,
            detail,
            ts_ms,
        });
    }
}

/// One strategy log line relayed from the backend (`self.log.*()`), Phase 10
/// log-output Open Question. Kept in a small ring buffer and shown in the Live Run
/// Panel; a dedicated filterable viewer is Phase 11.
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyLogLine {
    pub run_id: String,
    pub level: String,
    pub message: String,
    pub ts_ms: i64,
}

/// Phase 10 (Open Question: "Live Strategy のログ出力先"): the last `CAP` strategy
/// log lines, oldest-first. `backend_event_drain_system` pushes from a
/// `StrategyLogMessage`; the Live Run Panel renders the most recent few.
#[derive(Resource, Default, Debug, Clone)]
pub struct StrategyLogs {
    pub lines: std::collections::VecDeque<StrategyLogLine>,
}

impl StrategyLogs {
    /// Last 100 lines (plan: "直近 100 行").
    pub const CAP: usize = 100;

    pub fn push(&mut self, run_id: String, level: String, message: String, ts_ms: i64) {
        self.lines.push_back(StrategyLogLine {
            run_id,
            level,
            message,
            ts_ms,
        });
        while self.lines.len() > Self::CAP {
            self.lines.pop_front();
        }
    }

    /// The most recent `n` lines, oldest-first (so callers render top→bottom in
    /// chronological order). Fewer than `n` if the buffer is shorter.
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &StrategyLogLine> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip)
    }
}

/// Phase 9 §3.8: one UI order whose state became unknown after a backend restart
/// (the optimistic record exists locally but the freshly-restarted backend does
/// not track it as working). Shown in the reconcile modal.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileUnknownOrder {
    pub client_order_id: String,
    pub symbol: String,
    pub status: String,
}

/// Phase 9 §3.8: drives the post-restart reconcile modal. Populated by
/// `apply_status_update` from an `OrdersReconciled` diff; the modal opens while
/// `unknown` is non-empty and clears on user dismiss (it is a notification — the
/// user re-checks orders via the venue after re-login).
#[derive(Resource, Default, Debug, Clone)]
pub struct ReconcilePrompt {
    pub unknown: Vec<ReconcileUnknownOrder>,
}

/// Nautilus terminal `OrderStatus` names (mirrors the backend facade's
/// `_TERMINAL_STATUSES`). A terminal order is settled, so it is never part of a
/// reconcile diff.
pub fn is_terminal_order_status(status: &str) -> bool {
    matches!(
        status,
        "FILLED" | "CANCELED" | "REJECTED" | "EXPIRED" | "DENIED"
    )
}

/// Phase 9 §3.8: orders the UI optimistically believes are still working but the
/// backend (`backend_client_order_ids` from `GetOrders`) does not track. After an
/// auto-restart the fresh backend has no session, so every working UI order is
/// flagged "state unknown" until the user re-logs in and re-checks.
pub fn reconcile_unknown_orders(
    live: &LiveOrders,
    backend_client_order_ids: &[String],
) -> Vec<ReconcileUnknownOrder> {
    live.orders
        .iter()
        .filter(|o| !is_terminal_order_status(&o.status))
        .filter(|o| {
            !backend_client_order_ids
                .iter()
                .any(|id| id == &o.client_order_id)
        })
        .map(|o| ReconcileUnknownOrder {
            client_order_id: o.client_order_id.clone(),
            symbol: o.symbol.clone(),
            status: o.status.clone(),
        })
        .collect()
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

    fn make_live_order(client_order_id: &str) -> LiveOrder {
        LiveOrder {
            client_order_id: client_order_id.to_string(),
            symbol: "7203.T".to_string(),
            side: "BUY".to_string(),
            qty: 100.0,
            price: Some(2500.0),
            status: "SUBMITTED".to_string(),
            ..Default::default()
        }
    }

    fn make_live_order_with_status(client_order_id: &str, status: &str) -> LiveOrder {
        let mut o = make_live_order(client_order_id);
        o.status = status.to_string();
        o
    }

    #[test]
    fn reconcile_flags_working_orders_absent_from_backend() {
        // c1 working + tracked by backend → ok; c2 working + NOT tracked → unknown.
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order_with_status("c1", "ACCEPTED"));
        lo.upsert_full(make_live_order_with_status("c2", "ACCEPTED"));
        let unknown = reconcile_unknown_orders(&lo, &["c1".to_string()]);
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].client_order_id, "c2");
        assert_eq!(unknown[0].symbol, "7203.T");
    }

    #[test]
    fn reconcile_ignores_terminal_orders() {
        // A FILLED/CANCELED order is settled; its absence from the backend is not a
        // reconcile concern even though the backend doesn't list it.
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order_with_status("filled", "FILLED"));
        lo.upsert_full(make_live_order_with_status("canceled", "CANCELED"));
        assert!(reconcile_unknown_orders(&lo, &[]).is_empty());
    }

    #[test]
    fn reconcile_empty_backend_flags_all_working() {
        // Post-restart fresh backend tracks nothing → every working order unknown.
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order_with_status("c1", "ACCEPTED"));
        lo.upsert_full(make_live_order_with_status("c2", "PARTIALLY_FILLED"));
        let unknown = reconcile_unknown_orders(&lo, &[]);
        assert_eq!(unknown.len(), 2);
    }

    #[test]
    fn reconcile_all_known_is_empty() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order_with_status("c1", "ACCEPTED"));
        let unknown = reconcile_unknown_orders(&lo, &["c1".to_string(), "extra".to_string()]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn live_orders_upsert_full_inserts_newest_first() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        lo.upsert_full(make_live_order("c2"));
        assert_eq!(lo.orders.len(), 2);
        assert_eq!(lo.orders[0].client_order_id, "c2", "newest first");
        assert_eq!(lo.orders[1].client_order_id, "c1");
    }

    #[test]
    fn live_orders_upsert_full_replaces_same_id() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        let mut updated = make_live_order("c1");
        updated.qty = 200.0;
        lo.upsert_full(updated);
        assert_eq!(lo.orders.len(), 1, "same client_order_id replaces, no dup");
        assert_eq!(lo.orders[0].qty, 200.0);
    }

    #[test]
    fn live_orders_apply_event_preserves_static_fields() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        lo.apply_event("c1", "V123", "FILLED", 100.0, 2501.0, 42, "");
        let o = &lo.orders[0];
        // status/fill updated from the event...
        assert_eq!(o.status, "FILLED");
        assert_eq!(o.filled_qty, 100.0);
        assert_eq!(o.avg_price, 2501.0);
        assert_eq!(o.venue_order_id, "V123");
        assert_eq!(o.ts_ms, 42);
        // ...while symbol/side/qty/price (only known from PlaceOrder) survive.
        assert_eq!(o.symbol, "7203.T");
        assert_eq!(o.side, "BUY");
        assert_eq!(o.qty, 100.0);
        assert_eq!(o.price, Some(2500.0));
    }

    #[test]
    fn live_orders_apply_event_inserts_unknown_id_with_empty_static_fields() {
        let mut lo = LiveOrders::default();
        lo.apply_event("ghost", "V9", "ACCEPTED", 0.0, 0.0, 7, "");
        assert_eq!(lo.orders.len(), 1);
        let o = &lo.orders[0];
        assert_eq!(o.client_order_id, "ghost");
        assert_eq!(o.status, "ACCEPTED");
        assert!(
            o.symbol.is_empty(),
            "unknown order has no static fields yet"
        );
    }

    #[test]
    fn live_orders_apply_modify_updates_qty_price_and_preserves_symbol_side() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        lo.apply_modify(
            "c1",
            "V77",
            Some(300.0),
            Some(2600.0),
            "ACCEPTED",
            0.0,
            0.0,
            99,
        );
        let o = &lo.orders[0];
        assert_eq!(o.qty, 300.0, "new_qty overwrites");
        assert_eq!(o.price, Some(2600.0), "new_price overwrites");
        assert_eq!(o.status, "ACCEPTED");
        assert_eq!(o.venue_order_id, "V77");
        assert_eq!(o.ts_ms, 99);
        assert_eq!(o.symbol, "7203.T");
        assert_eq!(o.side, "BUY");
    }

    #[test]
    fn live_orders_apply_modify_keeps_unchanged_fields_when_none() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1")); // qty=100, price=Some(2500)
        lo.apply_modify("c1", "", None, Some(2700.0), "ACCEPTED", 0.0, 0.0, 5);
        let o = &lo.orders[0];
        assert_eq!(o.qty, 100.0, "None new_qty keeps the original qty");
        assert_eq!(o.price, Some(2700.0));
        assert_eq!(o.venue_order_id, "", "empty venue_order_id must not clear");
    }

    #[test]
    fn live_orders_apply_modify_unknown_id_is_noop() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        lo.apply_modify(
            "ghost",
            "V9",
            Some(900.0),
            Some(1.0),
            "ACCEPTED",
            0.0,
            0.0,
            1,
        );
        assert_eq!(lo.orders.len(), 1, "unknown id must not insert");
        assert_eq!(lo.orders[0].client_order_id, "c1");
        assert_eq!(lo.orders[0].qty, 100.0, "existing order is untouched");
    }

    #[test]
    fn live_orders_caps_retention() {
        let mut lo = LiveOrders::default();
        for i in 0..(MAX_LIVE_ORDERS + 20) {
            lo.upsert_full(make_live_order(&format!("c{i}")));
        }
        assert_eq!(
            lo.orders.len(),
            MAX_LIVE_ORDERS,
            "retention must be capped to bound memory / per-frame work"
        );
        // 最新 (最後に入れた) が先頭に残り、最古が落ちる。
        assert_eq!(
            lo.orders[0].client_order_id,
            format!("c{}", MAX_LIVE_ORDERS + 19)
        );
    }

    #[test]
    fn live_orders_apply_event_keeps_existing_venue_id_when_event_blank() {
        let mut lo = LiveOrders::default();
        let mut seeded = make_live_order("c1");
        seeded.venue_order_id = "V1".to_string();
        lo.upsert_full(seeded);
        lo.apply_event("c1", "", "PARTIALLY_FILLED", 50.0, 2500.0, 9, "");
        assert_eq!(
            lo.orders[0].venue_order_id, "V1",
            "blank venue_order_id in event must not wipe the known id"
        );
    }

    #[test]
    fn live_orders_apply_event_fill_is_monotonic_cancel_keeps_partial() {
        // §3.12 regression: a Tachibana cancel/modify RPC response carries
        // filled_qty=0.0. Routed via OrderStatusUpdated -> apply_event it must NOT
        // clobber a prior partial fill (recorded by the EC stream), or a real-money
        // position is under-reported. Cumulative fill never legitimately decreases.
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        // EC stream recorded a 40-share partial fill at 2502.0.
        lo.apply_event("c1", "V123", "PARTIALLY_FILLED", 40.0, 2502.0, 10, "");
        // Cancel RPC response comes back with filled=0.0.
        lo.apply_event("c1", "V123", "CANCELED", 0.0, 0.0, 20, "");
        let o = &lo.orders[0];
        assert_eq!(o.status, "CANCELED", "status always updates");
        assert_eq!(o.filled_qty, 40.0, "monotonic: downward fill is ignored");
        assert_eq!(
            o.avg_price, 2502.0,
            "avg_price kept with the fill it belongs to"
        );
        assert_eq!(o.ts_ms, 20, "ts_ms always updates");
    }

    #[test]
    fn live_orders_apply_event_fill_advances_when_increasing() {
        // Forward progress (40 -> 100) must overwrite filled/avg as before.
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        lo.apply_event("c1", "V123", "PARTIALLY_FILLED", 40.0, 2502.0, 10, "");
        lo.apply_event("c1", "V123", "FILLED", 100.0, 2505.0, 20, "");
        let o = &lo.orders[0];
        assert_eq!(o.filled_qty, 100.0);
        assert_eq!(o.avg_price, 2505.0);
        assert_eq!(o.status, "FILLED");
    }

    #[test]
    fn live_orders_apply_modify_fill_is_monotonic() {
        // §3.12 regression (symmetric with apply_event): a modify ACK (Tachibana
        // CLMKabuCorrectOrder / kabu remap) reporting only the new leg's fill must
        // NOT clobber a larger partial already recorded by the EC stream.
        let mut lo = LiveOrders::default();
        lo.upsert_full(make_live_order("c1"));
        lo.apply_event("c1", "V123", "PARTIALLY_FILLED", 40.0, 2502.0, 10, "");
        // Modify ACCEPTED response comes back reporting filled=0.0 (new leg).
        lo.apply_modify("c1", "V456", Some(60.0), None, "ACCEPTED", 0.0, 0.0, 20);
        let o = &lo.orders[0];
        assert_eq!(o.status, "ACCEPTED", "status always updates");
        assert_eq!(o.qty, 60.0, "new_qty applied");
        assert_eq!(o.filled_qty, 40.0, "monotonic: downward fill is ignored");
        assert_eq!(
            o.avg_price, 2502.0,
            "avg_price kept with the fill it belongs to"
        );
        assert_eq!(o.venue_order_id, "V456", "remapped venue_order_id updates");
        assert_eq!(o.ts_ms, 20, "ts_ms always updates");
    }

    #[test]
    fn live_runs_apply_event_inserts_and_fixes_start_ts() {
        let mut lr = LiveRuns::default();
        lr.apply_event("run-1", "strat-a", "READY", 100);
        lr.apply_event("run-1", "strat-a", "RUNNING", 200);
        assert_eq!(lr.runs.len(), 1, "same run_id upserts, no dup");
        let r = &lr.runs[0];
        assert_eq!(r.status, "RUNNING", "status refreshes");
        assert_eq!(r.started_ts_ms, 100, "start ts is fixed at first event");
        assert_eq!(r.updated_ts_ms, 200, "updated ts advances");
    }

    #[test]
    fn live_runs_blank_strategy_id_does_not_clear() {
        let mut lr = LiveRuns::default();
        lr.apply_event("run-1", "strat-a", "RUNNING", 100);
        lr.apply_event("run-1", "", "PAUSED", 200);
        assert_eq!(
            lr.runs[0].strategy_id, "strat-a",
            "an empty strategy_id must not wipe a known one"
        );
    }

    #[test]
    fn is_terminal_run_status_classifies_states() {
        assert!(is_terminal_run_status("STOPPED"));
        assert!(is_terminal_run_status("ERROR"));
        assert!(!is_terminal_run_status("RUNNING"));
        assert!(!is_terminal_run_status("PAUSED"));
    }

    #[test]
    fn live_runs_caps_retention() {
        let mut lr = LiveRuns::default();
        for i in 0..(MAX_LIVE_RUNS + 5) {
            lr.apply_event(&format!("run-{i}"), "s", "RUNNING", i as i64);
        }
        assert_eq!(lr.runs.len(), MAX_LIVE_RUNS, "retention is capped");
    }

    // ── Phase 10 §2.9: strategy_id merge + OrdersFilter ──────────────────────

    #[test]
    fn apply_event_strategy_id_merge_invariant() {
        let mut lo = LiveOrders::default();
        // Unary PlaceOrder seeds MANUAL-001.
        let mut seeded = make_live_order("c1");
        seeded.strategy_id = "MANUAL-001".to_string();
        lo.upsert_full(seeded);
        // A later untagged EC-stream OrderEvent (strategy_id="") must NOT clear it.
        lo.apply_event("c1", "V1", "PARTIALLY_FILLED", 10.0, 2500.0, 5, "");
        assert_eq!(
            lo.orders[0].strategy_id, "MANUAL-001",
            "empty strategy_id must not wipe a known tag"
        );
        // A non-empty value overwrites.
        lo.apply_event("c1", "V1", "FILLED", 100.0, 2500.0, 6, "LIVE-abc12345");
        assert_eq!(lo.orders[0].strategy_id, "LIVE-abc12345", "non-empty wins");
    }

    #[test]
    fn apply_event_inserts_unknown_with_received_strategy_id() {
        let mut lo = LiveOrders::default();
        lo.apply_event("ghost", "V9", "ACCEPTED", 0.0, 0.0, 7, "LIVE-deadbeef");
        assert_eq!(lo.orders[0].strategy_id, "LIVE-deadbeef");
    }

    #[test]
    fn live_runs_apply_telemetry_upserts_before_lifecycle() {
        let mut lr = LiveRuns::default();
        // Telemetry races ahead of any lifecycle event → creates the row.
        lr.apply_telemetry("run-1", "LIVE-abc", 100.0, 50.0, 3, 1, 500);
        assert_eq!(lr.runs.len(), 1);
        let r = &lr.runs[0];
        assert_eq!(r.realized_pnl, 100.0);
        assert_eq!(r.order_count, 3);
        assert_eq!(r.started_ts_ms, 500, "started fixed at first event seen");
        assert_eq!(r.status, "", "telemetry carries no lifecycle status");
        // A later lifecycle event fills status without clobbering started_ts_ms.
        lr.apply_event("run-1", "LIVE-abc", "RUNNING", 600);
        assert_eq!(lr.runs[0].status, "RUNNING");
        assert_eq!(lr.runs[0].started_ts_ms, 500);
        assert_eq!(
            lr.runs[0].realized_pnl, 100.0,
            "lifecycle must not reset PnL"
        );
    }

    #[test]
    fn live_runs_apply_telemetry_after_lifecycle_updates_counters_only() {
        let mut lr = LiveRuns::default();
        lr.apply_event("run-1", "LIVE-abc", "RUNNING", 100);
        lr.apply_telemetry("run-1", "LIVE-abc", 2000.0, -300.0, 5, 4, 200);
        let r = &lr.runs[0];
        assert_eq!(r.status, "RUNNING", "telemetry must not touch status");
        assert_eq!(r.started_ts_ms, 100);
        assert_eq!(r.realized_pnl, 2000.0);
        assert_eq!(r.unrealized_pnl, -300.0);
        assert_eq!(r.order_count, 5);
        assert_eq!(r.fill_count, 4);
    }

    #[test]
    fn live_runs_apply_telemetry_empty_strategy_id_does_not_clear() {
        let mut lr = LiveRuns::default();
        lr.apply_telemetry("run-1", "LIVE-abc", 0.0, 0.0, 0, 0, 100);
        lr.apply_telemetry("run-1", "", 10.0, 0.0, 1, 0, 200);
        assert_eq!(
            lr.runs[0].strategy_id, "LIVE-abc",
            "empty strategy_id must not wipe a known one"
        );
    }

    #[test]
    fn order_matches_filter_all_manual_strategy() {
        assert!(order_matches_filter("MANUAL-001", &OrdersFilter::All));
        assert!(order_matches_filter("", &OrdersFilter::All));
        assert!(order_matches_filter("MANUAL-001", &OrdersFilter::Manual));
        assert!(!order_matches_filter("LIVE-abc", &OrdersFilter::Manual));
        assert!(order_matches_filter(
            "LIVE-abc",
            &OrdersFilter::Strategy("LIVE-abc".to_string())
        ));
        assert!(!order_matches_filter(
            "LIVE-xyz",
            &OrdersFilter::Strategy("LIVE-abc".to_string())
        ));
    }

    fn order_tagged(client_order_id: &str, strategy_id: &str) -> LiveOrder {
        let mut o = make_live_order(client_order_id);
        o.strategy_id = strategy_id.to_string();
        o
    }

    #[test]
    fn filtered_view_and_distinct_ids() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(order_tagged("c1", "MANUAL-001"));
        lo.upsert_full(order_tagged("c2", "LIVE-abc"));
        lo.upsert_full(order_tagged("c3", "MANUAL-001"));
        // distinct (newest-first storage): c3 MANUAL, c2 LIVE, c1 MANUAL → [MANUAL-001, LIVE-abc]
        assert_eq!(
            lo.distinct_strategy_ids(),
            vec!["MANUAL-001".to_string(), "LIVE-abc".to_string()]
        );
        let manual = lo.filtered(&OrdersFilter::Manual);
        assert_eq!(manual.len(), 2);
        assert!(manual.iter().all(|o| o.strategy_id == "MANUAL-001"));
        let live = lo.filtered(&OrdersFilter::Strategy("LIVE-abc".to_string()));
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].client_order_id, "c2");
        assert_eq!(lo.filtered(&OrdersFilter::All).len(), 3);
    }

    #[test]
    fn next_filter_cycles_all_manual_strategy_wrap() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(order_tagged("c1", "MANUAL-001"));
        lo.upsert_full(order_tagged("c2", "LIVE-abc"));
        // Cycle: All → Manual → Strategy(LIVE-abc) → All
        let f0 = OrdersFilter::All;
        let f1 = next_filter(&f0, &lo);
        assert_eq!(f1, OrdersFilter::Manual);
        let f2 = next_filter(&f1, &lo);
        assert_eq!(f2, OrdersFilter::Strategy("LIVE-abc".to_string()));
        let f3 = next_filter(&f2, &lo);
        assert_eq!(f3, OrdersFilter::All, "wraps back to All");
    }

    #[test]
    fn next_filter_no_manual_skips_manual_option() {
        let mut lo = LiveOrders::default();
        lo.upsert_full(order_tagged("c1", "LIVE-abc"));
        // Only All → Strategy(LIVE-abc) → All (no MANUAL-001 present).
        let f1 = next_filter(&OrdersFilter::All, &lo);
        assert_eq!(f1, OrdersFilter::Strategy("LIVE-abc".to_string()));
        assert_eq!(next_filter(&f1, &lo), OrdersFilter::All);
    }

    #[test]
    fn next_filter_dropped_current_falls_back() {
        // current = a strategy no longer in the book → advance to first real option.
        let lo = LiveOrders::default(); // empty → cycle is just [All]
        let stale = OrdersFilter::Strategy("LIVE-gone".to_string());
        assert_eq!(next_filter(&stale, &lo), OrdersFilter::All);
    }

    #[test]
    fn filter_label_renders_each_variant() {
        assert_eq!(filter_label(&OrdersFilter::All), "All");
        assert_eq!(filter_label(&OrdersFilter::Manual), "Manual");
        // "LIVE-abcdef1234" is 15 chars; short_id(.., 8) tails to the last 8 = "cdef1234".
        assert_eq!(
            filter_label(&OrdersFilter::Strategy("LIVE-abcdef1234".to_string())),
            "Strategy: …cdef1234"
        );
    }

    #[test]
    fn short_id_keeps_short_and_truncates_long() {
        assert_eq!(short_id("abc", 6), "abc");
        assert_eq!(short_id("strat-deadbeef0011", 8), "…beef0011");
        assert_eq!(short_id("strat-deadbeef0011", 8).chars().count(), 9);
    }

    #[test]
    fn secret_prompt_close_clears_active_and_error() {
        // §3.10: close() is the single choke point — it must null BOTH fields so a
        // stale submit error never lingers for the next prompt.
        let mut p = SecretPrompt {
            active: Some(SecretPromptRequest {
                request_id: "r1".to_string(),
                venue: "TACHIBANA".to_string(),
                kind: "second_password".to_string(),
                purpose: "new_order".to_string(),
            }),
            error: Some("SECOND_SECRET_INVALID".to_string()),
        };
        p.close();
        assert!(p.active.is_none(), "close must drop the active request");
        assert!(p.error.is_none(), "close must drop the stale error");
    }

    #[test]
    fn redacted_secret_debug_does_not_leak_plaintext() {
        let s = RedactedSecret::new("hunter2".to_string());
        assert_eq!(format!("{s:?}"), "RedactedSecret(***)");
        assert_eq!(s.expose(), "hunter2");
        // The command embedding it must also redact.
        let cmd = TransportCommand::SubmitSecret {
            request_id: "r1".to_string(),
            secret: s,
        };
        let dbg = format!("{cmd:?}");
        assert!(
            !dbg.contains("hunter2"),
            "secret must never appear in Debug: {dbg}"
        );
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
            list: vec![Ticker {
                id: "7203.TSE".into(),
                name: "Toyota".into(),
                market: "TSE".into(),
            }],
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
            list: vec![Ticker {
                id: "OLD.TSE".into(),
                name: "Old".into(),
                market: "TSE".into(),
            }],
            source: TickersSource::Unknown,
            status: TickersStatus::InFlight,
        };
        let new_instruments = vec![
            Ticker {
                id: "1301.TSE".into(),
                name: "Kyokuyo".into(),
                market: "TSE".into(),
            },
            Ticker {
                id: "7203.TSE".into(),
                name: "Toyota".into(),
                market: "TSE".into(),
            },
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
        let stale = vec![Ticker {
            id: "7203.TSE".into(),
            name: "Toyota".into(),
            market: "TSE".into(),
        }];
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
        assert_eq!(
            tickers_source_to_wire(TickersSource::ReplayCatalogFallback),
            Some("local".to_string())
        );
        assert_eq!(
            tickers_source_to_wire(TickersSource::LocalVenueSnapshot),
            Some("local".to_string())
        );
        assert_eq!(
            tickers_source_to_wire(TickersSource::LiveVenue),
            Some("live".to_string())
        );
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
