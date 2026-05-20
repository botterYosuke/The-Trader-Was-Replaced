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
    AccountPosition, AvailableInstruments, BackendEvent, BackendStartupStage, BackendStatus,
    BackendStatusUpdate, ExecutionModeRes, LastPrices, LastRunResult, LiveOrder, LiveOrders,
    OrderFeedback, PortfolioPosition, PortfolioState, ReloginPrompt, RunState, SecretPrompt,
    SecretPromptRequest,
    Tickers, TickersStatus, VenueStatusRes, parse_summary_json,
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

#[allow(clippy::too_many_arguments)]
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
    mut live_orders: ResMut<LiveOrders>,
    mut order_feedback: ResMut<OrderFeedback>,
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
            &mut live_orders,
            &mut order_feedback,
        );
    }
}

/// Receiver half of the backend-event channel (order/account/secret/logout).
#[derive(Resource)]
pub struct BackendEventChannel {
    pub rx: mpsc::UnboundedReceiver<BackendEvent>,
}

pub fn backend_event_drain_system(
    mut channel: ResMut<BackendEventChannel>,
    mut secret_prompt: ResMut<SecretPrompt>,
    mut live_orders: ResMut<LiveOrders>,
    mut portfolio: ResMut<PortfolioState>,
    mut relogin_prompt: ResMut<ReloginPrompt>,
) {
    while let Ok(event) = channel.rx.try_recv() {
        match event {
            BackendEvent::SecretRequired {
                request_id,
                venue,
                kind,
                purpose,
            } => {
                // Phase 9 §3.10: open the SecretModal. Tachibana only — kabu/mock
                // never push this. A new request supersedes any stale prompt.
                info!(
                    "[backend-event] SecretRequired request_id={request_id} venue={venue} kind={kind} purpose={purpose}"
                );
                secret_prompt.active = Some(SecretPromptRequest {
                    request_id,
                    venue,
                    kind,
                    purpose,
                });
            }
            BackendEvent::OrderEvent {
                order_id,
                venue_order_id,
                client_order_id,
                status,
                filled_qty,
                avg_price,
                ts_ms,
            } => {
                // Phase 9 §3.12 (entry side): merge status/fill into the order the
                // PlaceOrder response seeded.
                info!(
                    "[backend-event] OrderEvent order_id={order_id} venue_order_id={venue_order_id} client_order_id={client_order_id} status={status} filled_qty={filled_qty} avg_price={avg_price} ts_ms={ts_ms}"
                );
                live_orders.apply_event(
                    &client_order_id,
                    &venue_order_id,
                    &status,
                    filled_qty,
                    avg_price,
                    ts_ms,
                );
            }
            BackendEvent::AccountEvent {
                cash,
                buying_power,
                positions,
                ts_ms,
            } => {
                // Phase 9 §3.12 / §3.4: reduce the account snapshot push into the
                // shared `PortfolioState` so BuyingPowerPanel / PositionsPanel show
                // Live data through the existing display path (same resource as the
                // Replay `PortfolioLoaded` reducer).
                info!(
                    "[backend-event] AccountEvent cash={cash} buying_power={buying_power} positions={} ts_ms={ts_ms}",
                    positions.len()
                );
                apply_account_event(&mut portfolio, cash, buying_power, positions);
            }
            BackendEvent::VenueLogoutDetected { venue } => {
                // Phase 9 §3.5 / Step 7: open the ReloginModal. kabu 本体早朝ログアウト
                // (VenueHealthWatchdog) / Tachibana 閉局 (SS frame) のどちらでも届く。
                // モーダルはユーザーに再ログインを促すのみ (実際の再ログインは Venue メニュー)。
                info!("[backend-event] VenueLogoutDetected venue={venue}");
                relogin_prompt.active = Some(venue);
            }
        }
    }
}

/// Reduce a Live `AccountEvent` push into `PortfolioState` (Phase 9 §3.4 / §3.12).
///
/// The proto `AccountEvent` carries `cash` / `buying_power` / `positions` but
/// **no** `equity` field, so equity is derived as
/// `cash + Σ(qty * avg_price + unrealized_pnl)` — i.e. the cash balance plus each
/// holding's market value approximated by its book cost (`qty * avg_price`) plus
/// its unrealized P&L. This is an approximation: when `unrealized_pnl` already
/// reflects the gap between cost and live price, `avg_price + (live − avg) =
/// live`, so the sum equals true market value; it is exact whenever the venue's
/// `unrealized_pnl` is computed against the same `avg_price` reported here.
/// Marks `loaded = true` so panels switch out of the "No run yet" state.
pub fn apply_account_event(
    portfolio: &mut PortfolioState,
    cash: f64,
    buying_power: f64,
    positions: Vec<AccountPosition>,
) {
    portfolio.cash = cash;
    portfolio.buying_power = buying_power;
    portfolio.positions = positions
        .into_iter()
        .map(|p| PortfolioPosition {
            symbol: p.symbol,
            qty: p.qty,
            avg_price: p.avg_price,
            unrealized_pnl: p.unrealized_pnl,
        })
        .collect();
    portfolio.equity = cash
        + portfolio
            .positions
            .iter()
            .map(|p| p.qty as f64 * p.avg_price + p.unrealized_pnl)
            .sum::<f64>();
    portfolio.loaded = true;
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
    live_orders: &mut LiveOrders,
    order_feedback: &mut OrderFeedback,
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
            let mode_actually_changed = exec_mode.mode != mode;
            exec_mode.mode = mode;
            // Drop any stale order notice when the execution mode changes so a
            // reject from a prior venue/mode doesn't reappear out of context.
            order_feedback.message = None;
            // The account snapshot in PortfolioState is mode-specific: Live writes
            // it from `AccountEvent` (apply_account_event), Replay from a run's
            // `PortfolioLoaded`. BuyingPower/Positions panels read it unconditionally
            // (gated only on `loaded`), so without a reset the Live 余力/建玉 would
            // bleed into a Replay view (and vice versa) until the new mode happens
            // to repopulate it. Reset on a real change so panels fall back to the
            // "—"/"No run yet" state; the new mode then refills it.
            if mode_actually_changed {
                *portfolio = PortfolioState::default();
            }
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
        BackendStatusUpdate::OrderSeeded {
            client_order_id,
            venue_order_id,
            symbol,
            side,
            qty,
            price,
            status: order_status,
            filled_qty,
            avg_price,
            ts_ms,
        } => {
            live_orders.upsert_full(LiveOrder {
                client_order_id,
                venue_order_id,
                symbol,
                side,
                qty,
                price,
                status: order_status,
                filled_qty,
                avg_price,
                ts_ms,
            });
            // A successful place clears any prior reject/timeout notice.
            order_feedback.message = None;
        }
        BackendStatusUpdate::OrderStatusUpdated {
            client_order_id,
            venue_order_id,
            status: order_status,
            filled_qty,
            avg_price,
            ts_ms,
        } => {
            live_orders.apply_event(
                &client_order_id,
                &venue_order_id,
                &order_status,
                filled_qty,
                avg_price,
                ts_ms,
            );
        }
        BackendStatusUpdate::OrderModified {
            client_order_id,
            venue_order_id,
            new_qty,
            new_price,
            status: order_status,
            filled_qty,
            avg_price,
            ts_ms,
        } => {
            live_orders.apply_modify(
                &client_order_id,
                &venue_order_id,
                new_qty,
                new_price,
                &order_status,
                filled_qty,
                avg_price,
                ts_ms,
            );
            // A successful modify clears any prior reject/timeout notice.
            order_feedback.message = None;
        }
        BackendStatusUpdate::OrderRejected { action, error_code } => {
            // Surfaced to the user via the OrderPanel error line (no toast infra yet).
            order_feedback.message = Some(format!("{action}が拒否されました ({error_code})"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::BackendEvent;

    /// Phase 9 Step 7: VenueLogoutDetected push → ReloginPrompt.active がセットされ
    /// ReloginModal が開く (§3.5)。headless ハーネスと同じ縫い目で検証する。
    #[test]
    fn venue_logout_detected_opens_relogin_prompt() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut app = App::new();
        app.insert_resource(BackendEventChannel { rx });
        app.init_resource::<SecretPrompt>();
        app.init_resource::<LiveOrders>();
        app.init_resource::<PortfolioState>();
        app.init_resource::<ReloginPrompt>();
        app.add_systems(Update, backend_event_drain_system);

        tx.send(BackendEvent::VenueLogoutDetected {
            venue: "KABU".to_string(),
        })
        .unwrap();
        app.update();

        assert_eq!(
            app.world().resource::<ReloginPrompt>().active.as_deref(),
            Some("KABU"),
            "VenueLogoutDetected must open the relogin prompt with the venue id"
        );
    }
}
