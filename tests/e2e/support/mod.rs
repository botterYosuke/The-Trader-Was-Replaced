//! Headless E2E harness for the backend → ECS synchronization layer.
//!
//! Builds a `MinimalPlugins` Bevy `App` wired with the same three drain systems
//! the desktop binary runs (`backend_update_system` / `status_update_system` /
//! `backend_event_drain_system`) and all the trading resources they mutate.
//! Tests own the sender halves of the three channels, push `BackendStatusUpdate`
//! / `BackendTradingState` / `BackendEvent` onto them, pump `tick()`
//! (= `app.update()`), and assert the resulting resource state — the seam that
//! issue #4 asks for. See `tests/e2e/FLOWS.md`.

#![allow(dead_code)]

use bevy::prelude::*;
use bevy::MinimalPlugins;
use tokio::sync::mpsc;

use backcast::backend_sync::{
    backend_event_drain_system, status_update_system, BackendEventChannel, StatusUpdateChannel,
};
use backcast::replay::{ReplayStartupPhase, ReplayStartupProgress};
use backcast::trading::{
    backend_update_system, AvailableInstruments, BackendChannel, BackendEvent, BackendStatus,
    BackendStatusUpdate, BackendTradingState, ExecutionModeRes, InstrumentTradingDataMap,
    LastPrices, LastRunResult, PortfolioState, RunState, Tickers, TradingSession, TradingSettings,
    VenueStatusRes,
};

pub struct Harness {
    pub app: App,
    pub status_tx: mpsc::UnboundedSender<BackendStatusUpdate>,
    pub backend_tx: mpsc::UnboundedSender<BackendTradingState>,
    pub event_tx: mpsc::UnboundedSender<BackendEvent>,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_backend_enabled(true)
    }

    /// Build a harness with `backend_enabled = false` (simulation mode). The
    /// footer renders `grpc: DISABLED` and `backend_update_system` early-returns,
    /// so backend replay-clock pushes are no-ops. See G3 in `tests/e2e/FLOWS.md`.
    pub fn new_backend_disabled() -> Self {
        Self::with_backend_enabled(false)
    }

    fn with_backend_enabled(backend_enabled: bool) -> Self {
        let (status_tx, status_rx) = mpsc::unbounded_channel();
        let (backend_tx, backend_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        // Build explicitly instead of from_env() so the harness is env-independent.
        // backend_update_system early-returns when this is false (G3).
        app.insert_resource(TradingSettings {
            backend_enabled,
            backend_url: "http://127.0.0.1:0".to_string(),
            token: "test-token".to_string(),
            poll_interval_ms: 500,
            max_history_points: 1000,
            catalog_path: None,
        });

        app.insert_resource(StatusUpdateChannel { rx: status_rx });
        app.insert_resource(BackendChannel { rx: backend_rx });
        app.insert_resource(BackendEventChannel { rx: event_rx });

        app.insert_resource(BackendStatus::default())
            .insert_resource(LastRunResult::default())
            .insert_resource(ReplayStartupProgress::default())
            .insert_resource(AvailableInstruments::default())
            .insert_resource(PortfolioState::default())
            .insert_resource(VenueStatusRes::default())
            .insert_resource(ExecutionModeRes::default())
            .insert_resource(Tickers::default())
            .insert_resource(LastPrices::default())
            .insert_resource(TradingSession::default())
            .insert_resource(InstrumentTradingDataMap::default());

        app.add_systems(
            Update,
            (
                backend_update_system,
                status_update_system,
                backend_event_drain_system,
            ),
        );

        Self {
            app,
            status_tx,
            backend_tx,
            event_tx,
        }
    }

    /// Pump one frame so the drain systems run.
    pub fn tick(&mut self) {
        self.app.update();
    }

    /// Inject a backend status update (backend → ECS seam) and advance a frame.
    pub fn send_status(&mut self, update: BackendStatusUpdate) {
        self.status_tx.send(update).expect("status channel closed");
        self.tick();
    }

    /// Inject a backend event (order/account/secret/logout) and advance a frame.
    pub fn send_event(&mut self, event: BackendEvent) {
        self.event_tx.send(event).expect("event channel closed");
        self.tick();
    }

    /// Push a backend replay-clock state carrying `timestamp_ms` and advance a
    /// frame. Other `BackendTradingState` fields are left at their serde
    /// defaults — only the replay clock matters for the v1 flows.
    pub fn push_state(&mut self, timestamp_ms: i64) {
        let state: BackendTradingState = serde_json::from_value(serde_json::json!({
            "price": 0.0,
            "history": [],
            "timestamp": 0.0,
            "timestamp_ms": timestamp_ms,
        }))
        .expect("BackendTradingState fixture");
        self.backend_tx.send(state).expect("backend channel closed");
        self.tick();
    }

    pub fn run_state(&self) -> RunState {
        self.app.world().resource::<LastRunResult>().state.clone()
    }

    pub fn last_run(&self) -> LastRunResult {
        self.app.world().resource::<LastRunResult>().clone()
    }

    pub fn portfolio(&self) -> PortfolioState {
        self.app.world().resource::<PortfolioState>().clone()
    }

    pub fn timestamp_ms(&self) -> i64 {
        self.app.world().resource::<TradingSession>().timestamp_ms
    }

    pub fn venue(&self) -> VenueStatusRes {
        self.app.world().resource::<VenueStatusRes>().clone()
    }

    pub fn exec_mode(&self) -> ExecutionModeRes {
        self.app.world().resource::<ExecutionModeRes>().clone()
    }

    pub fn tickers(&self) -> Tickers {
        self.app.world().resource::<Tickers>().clone()
    }

    pub fn available(&self) -> AvailableInstruments {
        self.app.world().resource::<AvailableInstruments>().clone()
    }

    pub fn last_prices(&self) -> LastPrices {
        self.app.world().resource::<LastPrices>().clone()
    }

    pub fn startup_progress(&self) -> ReplayStartupProgress {
        self.app.world().resource::<ReplayStartupProgress>().clone()
    }

    /// `BackendStatus` does not derive `Clone`, so expose the flags directly.
    pub fn backend_connected(&self) -> bool {
        self.app.world().resource::<BackendStatus>().connected
    }

    pub fn backend_running(&self) -> bool {
        self.app.world().resource::<BackendStatus>().running
    }

    /// Footer renders `grpc: DISABLED` iff this is false (see footer.rs).
    pub fn backend_enabled(&self) -> bool {
        self.app.world().resource::<TradingSettings>().backend_enabled
    }

    pub fn backend_last_error(&self) -> Option<String> {
        self.app
            .world()
            .resource::<BackendStatus>()
            .last_error
            .clone()
    }

    /// Open a startup window so `ReplayStartup`/`RunComplete` correlation logic
    /// in `apply_status_update` is active (it no-ops unless `visible` and the
    /// `startup_id` matches).
    pub fn begin_startup(&mut self, startup_id: u64) {
        let mut progress = self
            .app
            .world_mut()
            .resource_mut::<ReplayStartupProgress>();
        progress.visible = true;
        progress.startup_id = startup_id;
        progress.phase = ReplayStartupPhase::Idle;
        progress.error = None;
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}
