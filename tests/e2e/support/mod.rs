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

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::MinimalPlugins;
use tokio::sync::mpsc;

use std::time::Duration;

use backcast::backend_sync::{
    backend_event_drain_system, status_update_system, BackendEventChannel, StatusUpdateChannel,
};
use backcast::replay::{ReplayStartupPhase, ReplayStartupProgress};
use backcast::ui::replay_startup_window::replay_startup_timeout_system;
use backcast::trading::{
    backend_update_system, AvailableInstruments, BackendChannel, BackendEvent, BackendStatus,
    BackendStatusUpdate, BackendTradingState, ExecutionMode, ExecutionModeRes,
    InstrumentTradingDataMap, LastPrices, LastRunResult, LiveOrders, OrderFeedback, PortfolioState,
    ReconcilePrompt, ReloginPrompt, ReplaySpeed, RunState, SecretPrompt, SelectedSymbol, Tickers,
    TradingSession, TradingSettings, TransportCommand, TransportCommandSender, VenueState,
    VenueStatusRes,
};

// Production UI input systems (mirror src/main.rs wiring).
use backcast::ui::components::{
    InstrumentRegistry, OpenMenu, PauseResumeButton, ScenarioMetadata, ScenarioStartupParams,
    ScenarioWritebackPaths, StrategyBuffer, StrategyRunRequested, RedoMenuRequested,
    UndoMenuRequested,
};
use backcast::ui::footer::{
    execution_mode_toggle_system, footer_pause_resume_system, speed_button_system,
    transport_button_system,
};
use backcast::ui::instrument_picker::{
    auto_fetch_available_on_replay_entry_system, auto_fetch_live_universe_on_connect_system,
};
use backcast::ui::instruments_universe_prune::unsubscribe_removed_instruments_system;
use backcast::ui::layout_persistence::{
    LayoutLoadDialogRequested, LayoutSaveAsRequested, LayoutSaveRequested,
};
use backcast::ui::menu_bar::{handle_strategy_run_system, menu_item_system};
use backcast::ui::order_panel::{
    confirm_modal_button_system, order_form_button_system, order_submit_button_system, OrderConfirm,
    OrderForm,
};
use backcast::ui::secret_modal::{secret_modal_button_system, secret_modal_input_system, SecretInput};
use backcast::ui::sidebar::{instrument_remove_button_system, instrument_row_click_system};
use backcast::ui::strategy_editor::StrategyAutoSaveState;

pub struct Harness {
    pub app: App,
    pub status_tx: mpsc::UnboundedSender<BackendStatusUpdate>,
    pub backend_tx: mpsc::UnboundedSender<BackendTradingState>,
    pub event_tx: mpsc::UnboundedSender<BackendEvent>,
    pub cmd_rx: mpsc::UnboundedReceiver<TransportCommand>,
    tmp: tempfile::TempDir,
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
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<TransportCommand>();

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
            .insert_resource(InstrumentTradingDataMap::default())
            .insert_resource(LiveOrders::default())
            .insert_resource(OrderFeedback::default())
            .insert_resource(ReconcilePrompt::default())
            .insert_resource(SecretPrompt::default())
            .insert_resource(ReloginPrompt::default());

        // Sender half of the UI → backend transport channel + every resource and
        // event the production UI input systems below take as a system param.
        // Mirrors the desktop binary's wiring (src/main.rs) so the click handlers
        // run unmodified. A missing one would panic *all* flows at system-param
        // validation, so this set must stay complete.
        app.insert_resource(TransportCommandSender { tx: cmd_tx })
            .insert_resource(ReplaySpeed::default())
            .insert_resource(SelectedSymbol::default())
            .insert_resource(StrategyBuffer::default())
            .insert_resource(StrategyAutoSaveState::default())
            .insert_resource(ScenarioMetadata::default())
            .insert_resource(ScenarioStartupParams::default())
            .insert_resource(ScenarioWritebackPaths::default())
            .insert_resource(InstrumentRegistry::default())
            .insert_resource(OpenMenu::default())
            .insert_resource(OrderForm::default())
            .insert_resource(OrderConfirm::default())
            .insert_resource(SecretInput::default())
            .insert_resource(ButtonInput::<KeyCode>::default());

        app.add_event::<StrategyRunRequested>()
            .add_event::<LayoutSaveRequested>()
            .add_event::<LayoutSaveAsRequested>()
            .add_event::<LayoutLoadDialogRequested>()
            .add_event::<UndoMenuRequested>()
            .add_event::<RedoMenuRequested>()
            .add_event::<KeyboardInput>();

        app.add_systems(
            Update,
            (
                backend_update_system,
                status_update_system,
                backend_event_drain_system,
                replay_startup_timeout_system,
            ),
        );

        // Production UI input systems. Producers are chained before their
        // consumers so a single `tick()` carries a click through to the command:
        //   PauseResume(Run) → StrategyRunRequested → handle_strategy_run → RunStrategy
        //   remove button → registry diff → unsubscribe command (needs a prior tick to prime)
        app.add_systems(
            Update,
            (
                (footer_pause_resume_system, handle_strategy_run_system).chain(),
                transport_button_system,
                speed_button_system,
                execution_mode_toggle_system,
                instrument_row_click_system,
                (
                    instrument_remove_button_system,
                    unsubscribe_removed_instruments_system,
                )
                    .chain(),
                menu_item_system,
                order_form_button_system,
                order_submit_button_system,
                confirm_modal_button_system,
                secret_modal_input_system,
                secret_modal_button_system,
                auto_fetch_live_universe_on_connect_system,
                auto_fetch_available_on_replay_entry_system,
            ),
        );

        Self {
            app,
            status_tx,
            backend_tx,
            event_tx,
            cmd_rx,
            tmp: tempfile::tempdir().expect("tempdir"),
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

    pub fn live_orders(&self) -> LiveOrders {
        self.app.world().resource::<LiveOrders>().clone()
    }

    pub fn order_feedback(&self) -> OrderFeedback {
        self.app.world().resource::<OrderFeedback>().clone()
    }

    pub fn secret_prompt(&self) -> SecretPrompt {
        self.app.world().resource::<SecretPrompt>().clone()
    }

    pub fn relogin_prompt(&self) -> ReloginPrompt {
        self.app.world().resource::<ReloginPrompt>().clone()
    }

    pub fn reconcile_prompt(&self) -> ReconcilePrompt {
        self.app.world().resource::<ReconcilePrompt>().clone()
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

    /// Open a startup window and arm the soft-timeout clock (`started_at_elapsed`
    /// = current `Time<Real>` elapsed). Use with [`advance_real_time`] to drive
    /// `replay_startup_timeout_system` (A9).
    pub fn arm_startup_timeout(&mut self, startup_id: u64) {
        let now = self.app.world().resource::<Time<Real>>().elapsed();
        let mut progress = self.app.world_mut().resource_mut::<ReplayStartupProgress>();
        progress.visible = true;
        progress.startup_id = startup_id;
        progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        progress.error = None;
        progress.started_at_elapsed = Some(now);
    }

    /// Advance the headless `Time<Real>` clock and pump one frame, so timer-driven
    /// systems (e.g. the startup soft-timeout) observe elapsed wall time.
    pub fn advance_real_time(&mut self, dur: Duration) {
        self.app.world_mut().resource_mut::<Time<Real>>().advance_by(dur);
        self.tick();
    }

    /// Mirror of the production Close-button reset (`replay_startup_close_button_system`).
    /// The button itself is UI-driven (Interaction), so headless tests invoke the
    /// state transition directly.
    pub fn close_startup_window(&mut self) {
        let mut progress = self.app.world_mut().resource_mut::<ReplayStartupProgress>();
        progress.visible = false;
        progress.phase = ReplayStartupPhase::Idle;
        progress.detail = None;
        progress.baseline_timestamp_ms = None;
        progress.started_at_elapsed = None;
        progress.start_engine_accepted = false;
        progress.error = None;
    }

    // ── Production UI driving ───────────────────────────────────────────────

    /// Drain every `TransportCommand` the UI systems emitted since the last call.
    pub fn drain_commands(&mut self) -> Vec<TransportCommand> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            out.push(cmd);
        }
        out
    }

    /// Spawn a clickable button carrying `marker`, mark it `Pressed`, and pump one
    /// frame. Newly added `Interaction` counts as `Changed`, so the production
    /// handler whose query matches `marker` fires exactly once — the same path a
    /// real mouse press takes. Each call is a fresh entity, so repeated clicks of
    /// the same button type re-trigger cleanly.
    pub fn click<M: Component>(&mut self, marker: M) {
        self.app
            .world_mut()
            .spawn((marker, Button, Interaction::Pressed));
        self.tick();
    }

    /// Set the mirrored replay state the footer transport buttons branch on
    /// (`RUNNING` → Pause, `PAUSED` → Resume/Step, otherwise → Run).
    pub fn set_replay_state(&mut self, state: Option<&str>) {
        self.app
            .world_mut()
            .resource_mut::<TradingSession>()
            .replay_state = state.map(|s| s.to_string());
    }

    /// Drive the production footer Run button end to end in Replay mode and
    /// return the `startup_id` the run-request chain assigned. Seeds a minimal
    /// valid scenario + cache path so `footer_pause_resume_system` →
    /// `handle_strategy_run_system` emits `RunStrategy` instead of blocking.
    pub fn run_via_ui(&mut self) -> u64 {
        self.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
        {
            let mut sc = self.app.world_mut().resource_mut::<ScenarioMetadata>();
            sc.instruments = vec!["7203.TSE".to_string()];
            sc.start = Some("2025-01-06".to_string());
            sc.end = Some("2025-03-31".to_string());
            sc.granularity = Some("Daily".to_string());
            sc.initial_cash = Some(1_000_000);
        }
        self.app
            .world_mut()
            .resource_mut::<InstrumentRegistry>()
            .editable = false;
        let cache_py = self.tmp.path().join("strategy_cache.py");
        std::fs::write(&cache_py, "# strategy cache fixture\n").expect("write cache fixture");
        self.app.world_mut().resource_mut::<StrategyBuffer>().cache_path = Some(cache_py);
        self.set_replay_state(None);
        self.app.world_mut().spawn((
            PauseResumeButton,
            Button,
            BackgroundColor::default(),
            Interaction::Pressed,
        ));
        self.tick();
        self.app
            .world()
            .resource::<ReplayStartupProgress>()
            .startup_id
    }

    /// Force the current execution mode (what the backend would have pushed). The
    /// mode toggle never writes this optimistically — see `e1`.
    pub fn set_exec_mode(&mut self, mode: ExecutionMode) {
        self.app.world_mut().resource_mut::<ExecutionModeRes>().mode = mode;
    }

    /// Set venue connection state — the precondition Live-mode UI gates on.
    pub fn set_venue(&mut self, state: VenueState, venue_id: &str) {
        let mut v = self.app.world_mut().resource_mut::<VenueStatusRes>();
        v.state = state;
        v.venue_id = Some(venue_id.to_string());
        v.configured_venue = Some(venue_id.to_string());
    }

    /// Set the scenario end date the replay-entry auto-fetch keys on.
    pub fn set_scenario_end(&mut self, end: &str) {
        self.app.world_mut().resource_mut::<ScenarioMetadata>().end = Some(end.to_string());
    }

    /// Seed the live-mode instrument registry (universe) for sidebar/prune flows.
    pub fn set_instruments(&mut self, ids: &[&str]) {
        let mut reg = self.app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.editable = true;
        reg.replace_all(&ids.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }

    /// Type plaintext into the open SecretModal via real keyboard events (the
    /// modal drains `Events<KeyboardInput>`). The prompt must already be active.
    pub fn type_secret(&mut self, text: &str) {
        {
            let mut kb = self.app.world_mut().resource_mut::<Events<KeyboardInput>>();
            for ch in text.chars() {
                kb.send(KeyboardInput {
                    key_code: KeyCode::KeyA,
                    logical_key: Key::Character(ch.to_string().into()),
                    state: ButtonState::Pressed,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            }
        }
        self.tick();
    }

    pub fn selected_symbol(&self) -> Option<String> {
        self.app.world().resource::<SelectedSymbol>().id.clone()
    }

    pub fn replay_speed(&self) -> u32 {
        self.app.world().resource::<ReplaySpeed>().current
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}
