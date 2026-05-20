//! Backend → ECS synchronization layer.
//!
//! Drains the two mpsc channels fed by the transport task (`BackendStatusUpdate`
//! and `BackendEvent`) and applies them to the observable Bevy resources
//! (`LastRunResult`, `PortfolioState`, `VenueStatusRes`, …). This is the heart
//! of the desktop app's state machine: every user-visible state transition that
//! originates from the backend flows through `apply_status_update`.
//!
//! Extracted from `main.rs` (the binary crate) into the library so headless
//! E2E tests can build a `MinimalPlugins` app, inject `BackendStatusUpdate`s on
//! the channel, pump `app.update()`, and assert the resulting resource state —
//! exactly the seam issue #4 asks for. See `tests/e2e/FLOWS.md`.

use crate::replay::{ReplayStartupPhase, ReplayStartupProgress};
use crate::trading::{
    AvailableInstruments, BackendEvent, BackendStartupStage, BackendStatus, BackendStatusUpdate,
    ExecutionModeRes, LastPrices, LastRunResult, PortfolioState, RunState, Tickers, TickersStatus,
    VenueStatusRes, parse_summary_json,
};
use bevy::prelude::*;
use chrono::NaiveDate;
use tokio::sync::mpsc;

/// Receiver half of the status-update channel. The sender lives in the transport
/// task (and in E2E tests, in the harness). Public `rx` so tests can construct
/// the resource directly from a channel they own.
#[derive(Resource)]
pub struct StatusUpdateChannel {
    pub rx: mpsc::UnboundedReceiver<BackendStatusUpdate>,
}

pub fn status_update_system(
    mut status: ResMut<BackendStatus>,
    mut channel: ResMut<StatusUpdateChannel>,
    mut last_run: ResMut<LastRunResult>,
    mut portfolio: ResMut<PortfolioState>,
    mut available: ResMut<AvailableInstruments>,
    mut progress: ResMut<ReplayStartupProgress>,
    mut venue_status: ResMut<VenueStatusRes>,
    mut exec_mode: ResMut<ExecutionModeRes>,
    mut tickers: ResMut<Tickers>,
    mut last_prices: ResMut<LastPrices>,
) {
    while let Ok(update) = channel.rx.try_recv() {
        apply_status_update(
            update,
            &mut status,
            &mut last_run,
            &mut portfolio,
            &mut available,
            &mut progress,
            &mut venue_status,
            &mut exec_mode,
            &mut tickers,
            &mut last_prices,
        );
    }
}

/// Receiver half of the backend-event channel (order/account/secret/logout).
#[derive(Resource)]
pub struct BackendEventChannel {
    pub rx: mpsc::UnboundedReceiver<BackendEvent>,
}

pub fn backend_event_drain_system(mut channel: ResMut<BackendEventChannel>) {
    while let Ok(event) = channel.rx.try_recv() {
        match event {
            BackendEvent::SecretRequired {
                request_id,
                venue,
                kind,
                purpose,
            } => info!(
                "[backend-event] SecretRequired request_id={request_id} venue={venue} kind={kind} purpose={purpose}"
            ),
            BackendEvent::OrderEvent {
                order_id,
                venue_order_id,
                client_order_id,
                status,
                filled_qty,
                avg_price,
                ts_ms,
            } => info!(
                "[backend-event] OrderEvent order_id={order_id} venue_order_id={venue_order_id} client_order_id={client_order_id} status={status} filled_qty={filled_qty} avg_price={avg_price} ts_ms={ts_ms}"
            ),
            BackendEvent::AccountEvent {
                cash,
                buying_power,
                positions,
                ts_ms,
            } => info!(
                "[backend-event] AccountEvent cash={cash} buying_power={buying_power} positions={} ts_ms={ts_ms}",
                positions.len()
            ),
            BackendEvent::VenueLogoutDetected { venue } => {
                info!("[backend-event] VenueLogoutDetected venue={venue}")
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_status_update(
    update: BackendStatusUpdate,
    status: &mut BackendStatus,
    last_run: &mut LastRunResult,
    portfolio: &mut PortfolioState,
    available: &mut AvailableInstruments,
    progress: &mut ReplayStartupProgress,
    venue_status: &mut VenueStatusRes,
    exec_mode: &mut ExecutionModeRes,
    tickers: &mut Tickers,
    last_prices: &mut LastPrices,
) {
    match update {
        BackendStatusUpdate::Connected(c) => status.connected = c,
        BackendStatusUpdate::Running(r) => status.running = r,
        BackendStatusUpdate::Error(e) => {
            status.last_error = Some(e);
            status.connected = false;
        }
        BackendStatusUpdate::RunStarted => {
            last_run.state = RunState::Running;
        }
        BackendStatusUpdate::ReplayStartup { startup_id, stage } => {
            if progress.visible && progress.startup_id == startup_id {
                progress.phase = match stage {
                    BackendStartupStage::ResettingReplay => ReplayStartupPhase::ResettingReplay,
                    BackendStartupStage::LoadingData => ReplayStartupPhase::LoadingData,
                    BackendStartupStage::StartingStrategy => ReplayStartupPhase::StartingStrategy,
                    BackendStartupStage::WaitingForFirstTick => {
                        ReplayStartupPhase::WaitingForFirstTick
                    }
                };
                if matches!(stage, BackendStartupStage::WaitingForFirstTick) {
                    progress.start_engine_accepted = true;
                }
            }
        }
        BackendStatusUpdate::RunComplete {
            startup_id,
            run_id,
            summary_json,
        } => {
            info!("RunComplete: run_id={} summary={}", run_id, summary_json);
            last_run.parsed_summary = parse_summary_json(&summary_json);
            last_run.run_id = Some(run_id);
            last_run.summary_json = Some(summary_json);
            last_run.state = RunState::Completed;

            if let Some(sid) = startup_id
                && progress.visible
                && progress.startup_id == sid
            {
                progress.visible = false;
                progress.phase = ReplayStartupPhase::Idle;
                progress.detail = None;
                progress.baseline_timestamp_ms = None;
                progress.started_at_elapsed = None;
                progress.start_engine_accepted = false;
            }
        }
        BackendStatusUpdate::RunFailed { startup_id, error } => {
            if let Some(sid) = startup_id
                && progress.visible
                && progress.startup_id == sid
            {
                progress.error = Some(error.clone());
            }
            last_run.state = RunState::Failed { error };
        }
        BackendStatusUpdate::PortfolioLoaded {
            buying_power,
            cash,
            equity,
            positions,
            orders,
        } => {
            portfolio.buying_power = buying_power;
            portfolio.cash = cash;
            portfolio.equity = equity;
            portfolio.positions = positions;
            portfolio.orders = orders;
            portfolio.loaded = true;
        }
        BackendStatusUpdate::AvailableInstrumentsLoaded { end_date, ids } => {
            apply_available_loaded(available, end_date, ids);
        }
        BackendStatusUpdate::AvailableInstrumentsFetchFailed { end_date, error } => {
            apply_available_failed(available, end_date, error);
        }
        BackendStatusUpdate::VenueChanged {
            state,
            venue_id,
            instruments_loaded,
        } => {
            venue_status.state = state;
            venue_status.venue_id = venue_id;
            venue_status.instruments_loaded = instruments_loaded;
        }
        BackendStatusUpdate::ExecutionModeChanged { mode } => {
            exec_mode.mode = mode;
        }
        BackendStatusUpdate::ConfiguredVenueDiscovered { venue_id } => {
            venue_status.configured_venue = venue_id;
        }
        BackendStatusUpdate::InstrumentsListStarted { source } => {
            tickers.source = source;
            tickers.status = TickersStatus::InFlight;
            // list is kept (shows stale data while in-flight)
        }
        BackendStatusUpdate::InstrumentsListed {
            source,
            instruments,
        } => {
            tickers.source = source;
            tickers.status = TickersStatus::Loaded;
            tickers.list = instruments;
        }
        BackendStatusUpdate::InstrumentsListFailed { source, error } => {
            tickers.source = source;
            tickers.status = TickersStatus::Failed(error);
            // list is kept (stale display)
        }
        BackendStatusUpdate::LastPricesUpdated { prices } => {
            last_prices.map = prices;
        }
    }
}

pub fn apply_available_loaded(
    available: &mut AvailableInstruments,
    end_date: NaiveDate,
    ids: Vec<String>,
) {
    available.by_end_date.insert(end_date, ids);
    available.in_flight.remove(&end_date);
}

pub fn apply_available_failed(
    available: &mut AvailableInstruments,
    end_date: NaiveDate,
    error: String,
) {
    available.last_error = Some((end_date, error));
    available.in_flight.remove(&end_date);
}
