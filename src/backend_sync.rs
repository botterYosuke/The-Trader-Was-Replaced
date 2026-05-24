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

use crate::backend_supervisor::{BackendLifecycle, BackendLifecycleHandle};
use crate::replay::{ReplayStartupPhase, ReplayStartupProgress};
use crate::trading::{
    AccountPosition, AvailableInstruments, BackendEvent, BackendStartupStage, BackendStatus,
    BackendStatusUpdate, ExecutionModeRes, LastPrices, LastRunResult, LiveOrder, LiveOrders,
    LiveRuns, OrderFeedback, PortfolioPosition, PortfolioState, PromoteFeedback, ReconcilePrompt,
    ReloginPrompt, RunState, SafetyToast, SecretPrompt, SecretPromptRequest, StrategyLogs, Tickers,
    TickersStatus, TransportCommand, TransportCommandSender, VenueStatusRes,
    is_terminal_order_status, parse_summary_json, reconcile_unknown_orders,
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
    mut reconcile_prompt: ResMut<ReconcilePrompt>,
    mut secret_prompt: ResMut<SecretPrompt>,
    mut promote_feedback: ResMut<PromoteFeedback>,
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
            &mut reconcile_prompt,
            &mut secret_prompt,
            &mut promote_feedback,
        );
    }
}

/// Receiver half of the backend-event channel (order/account/secret/logout).
#[derive(Resource)]
pub struct BackendEventChannel {
    pub rx: mpsc::UnboundedReceiver<BackendEvent>,
}

#[allow(clippy::too_many_arguments)]
pub fn backend_event_drain_system(
    mut channel: ResMut<BackendEventChannel>,
    mut secret_prompt: ResMut<SecretPrompt>,
    mut live_orders: ResMut<LiveOrders>,
    mut portfolio: ResMut<PortfolioState>,
    mut relogin_prompt: ResMut<ReloginPrompt>,
    mut live_runs: ResMut<LiveRuns>,
    mut safety_toast: ResMut<SafetyToast>,
    mut strategy_logs: ResMut<StrategyLogs>,
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
                // A fresh prompt supersedes any stale submit error.
                secret_prompt.error = None;
            }
            BackendEvent::OrderEvent {
                order_id,
                venue_order_id,
                client_order_id,
                status,
                filled_qty,
                avg_price,
                ts_ms,
                // Phase 10 §2.9 / M6: ordering subject's StrategyId — drives the
                // OrdersPanel filter. Merge invariant (apply_event): a non-empty
                // value tags the row; an empty EC-stream value never clears a
                // known MANUAL-001 / LIVE-.. tag.
                strategy_id,
            } => {
                // Phase 9 §3.12 (entry side): merge status/fill into the order the
                // PlaceOrder response seeded.
                info!(
                    "[backend-event] OrderEvent order_id={order_id} venue_order_id={venue_order_id} client_order_id={client_order_id} status={status} filled_qty={filled_qty} avg_price={avg_price} ts_ms={ts_ms} strategy_id={strategy_id}"
                );
                live_orders.apply_event(
                    &client_order_id,
                    &venue_order_id,
                    &status,
                    filled_qty,
                    avg_price,
                    ts_ms,
                    &strategy_id,
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
            // Phase 10 Step 3 (M8): live strategy telemetry. Step 3 only logs these —
            // the Run Badge / Live Run Panel (Steps 5-6) and the SafetyRailViolation
            // toast (Step 10) consume them once those panels exist.
            BackendEvent::LiveStrategyEvent {
                run_id,
                strategy_id,
                status,
                ts_ms,
            } => {
                info!(
                    "[backend-event] LiveStrategyEvent run_id={run_id} strategy_id={strategy_id} status={status} ts_ms={ts_ms}"
                );
                // §2.8: drive the Live Run Panel's run list.
                live_runs.apply_event(&run_id, &strategy_id, &status, ts_ms);
            }
            BackendEvent::SafetyRailViolation {
                run_id,
                kind,
                detail,
                ts_ms,
            } => {
                warn!(
                    "[backend-event] SafetyRailViolation run_id={run_id} kind={kind} detail={detail} ts_ms={ts_ms}"
                );
                // §2.10: surface the violation as a Footer toast (criterion line 484).
                safety_toast.show(run_id, kind, detail, ts_ms);
            }
            BackendEvent::StrategyLogMessage {
                run_id,
                level,
                message,
                ts_ms,
            } => {
                info!(
                    "[backend-event] StrategyLogMessage run_id={run_id} level={level} message={message} ts_ms={ts_ms}"
                );
                // Log Open Question: keep the last 100 lines for the Live Run Panel.
                strategy_logs.push(run_id, level, message, ts_ms);
            }
            // Phase 10 Step 7 (§2.8 / §2.9): run-scoped PnL / order / fill counters.
            BackendEvent::LiveStrategyTelemetry {
                run_id,
                strategy_id,
                realized_pnl,
                unrealized_pnl,
                order_count,
                fill_count,
                ts_ms,
            } => {
                info!(
                    "[backend-event] LiveStrategyTelemetry run_id={run_id} strategy_id={strategy_id} realized_pnl={realized_pnl} unrealized_pnl={unrealized_pnl} order_count={order_count} fill_count={fill_count} ts_ms={ts_ms}"
                );
                live_runs.apply_telemetry(
                    &run_id,
                    &strategy_id,
                    realized_pnl,
                    unrealized_pnl,
                    order_count,
                    fill_count,
                    ts_ms,
                );
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

/// supervisor lifecycle を footer 反映用の status update へ写像する。
/// `StartupFailed(code)` は loud な `Error` に（footer を `grpc: ERR` 赤に）、
/// それ以外は `None`（既存の grpc 表示ロジックに委ねる）。
pub fn lifecycle_status_update(
    s: BackendLifecycle,
) -> Option<crate::trading::BackendStatusUpdate> {
    match s {
        BackendLifecycle::StartupFailed(code) => Some(
            crate::trading::BackendStatusUpdate::Error(format!("backend startup failed: {code}")),
        ),
        _ => None,
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
    live_orders: &mut LiveOrders,
    order_feedback: &mut OrderFeedback,
    reconcile_prompt: &mut ReconcilePrompt,
    secret_prompt: &mut SecretPrompt,
    promote_feedback: &mut PromoteFeedback,
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
            strategy_id,
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
                // §2.9: the manual PlaceOrder response's MANUAL-001 tag (or "" from an
                // old producer). A non-empty tag here is preserved by apply_event's
                // empty-never-clears rule against later untagged EC-stream events.
                strategy_id,
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
                // A Cancel response carries no strategy_id; "" preserves the row's
                // existing tag (apply_event empty-never-clears invariant, §2.9).
                "",
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
        BackendStatusUpdate::OrderNotice { message } => {
            // Order-flow notice (incomplete success / transport error). Same bucket
            // as OrderRejected; surfaced verbatim in the OrderPanel feedback line.
            order_feedback.message = Some(message);
        }
        BackendStatusUpdate::SecretSubmitFailed { error_code } => {
            // §3.10: secret-flow failure surfaces in the SecretModal so the user can
            // retry — NOT in OrderFeedback (wrong bucket; would pop OrderPanel even
            // with no order flow and could be cleared by unrelated order updates).
            secret_prompt.error = Some(format!("第二暗証番号が拒否されました ({error_code})"));
        }
        BackendStatusUpdate::OrdersReconciled {
            backend_client_order_ids,
        } => {
            // §3.8: diff the UI's optimistic working orders against the backend's
            // truth. Any working order the backend no longer tracks is surfaced in
            // the reconcile modal (after a restart the fresh backend has none).
            reconcile_prompt.unknown =
                reconcile_unknown_orders(live_orders, &backend_client_order_ids);
        }
        BackendStatusUpdate::LiveStrategyPromoteResult {
            success,
            error_code,
            run_id,
        } => {
            // Phase 10 §2.7: surface the unary outcome of a PromoteToLive RPC chain.
            // Success also arrives as a pushed LiveStrategyEvent (Live Run Panel,
            // Step 6); here we only set the user-facing notice so a structured
            // reject is visible in LiveAuto (OrderFeedback would not be — its panel
            // is LiveManual-only).
            promote_feedback.message = Some(if success {
                format!("Live 戦略を起動しました (run: {run_id})")
            } else {
                format!("Promote to Live が拒否されました ({error_code})")
            });
        }
    }
}

/// Phase 9 §3.8: after the supervisor auto-restarts a crashed backend and reaches
/// `Ready` again, fire a `GetOrders` to reconcile the UI's optimistic order list
/// with the (fresh, session-less) backend's truth. Only fires on a Ready that
/// *follows* a Crashed (an actual restart — not the initial startup) and only
/// when there are working orders to reconcile. `Local` state tracks the previous
/// lifecycle and whether a crash has been seen since the last reconcile.
pub fn backend_restart_resync_system(
    lifecycle: Res<BackendLifecycleHandle>,
    live_orders: Res<LiveOrders>,
    venue_status: Res<VenueStatusRes>,
    sender: Option<Res<TransportCommandSender>>,
    mut saw_crash: Local<bool>,
    mut prev: Local<Option<BackendLifecycle>>,
) {
    let current = lifecycle.current();
    let changed = *prev != Some(current);
    *prev = Some(current);

    if matches!(current, BackendLifecycle::Crashed) {
        *saw_crash = true;
        return;
    }
    // Only act on the *transition* into Ready that follows a crash.
    if !(changed && current == BackendLifecycle::Ready && *saw_crash) {
        return;
    }
    *saw_crash = false;

    let has_working = live_orders
        .orders
        .iter()
        .any(|o| !is_terminal_order_status(&o.status));
    if !has_working {
        return; // nothing optimistic to reconcile
    }
    let Some(tx) = sender.as_ref() else {
        warn!("[backend] post-restart reconcile skipped: TransportCommandSender unavailable");
        return;
    };
    let venue = venue_status.venue_id.clone().unwrap_or_default();
    info!("[backend] auto-restart reached Ready — reconciling in-flight orders (GetOrders)");
    let _ = tx.tx.send(TransportCommand::GetOrders { venue });
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

    /// RED: `lifecycle_status_update` はまだ未定義（GREEN 段で追加）。
    /// StartupFailed はエラーコードを内包した `BackendStatusUpdate::Error` へ写す。
    #[test]
    fn lifecycle_status_update_maps_startup_failed_to_error() {
        let out = lifecycle_status_update(BackendLifecycle::StartupFailed(
            "BACKEND_VENUE_MISMATCH",
        ));
        match out {
            Some(crate::trading::BackendStatusUpdate::Error(msg)) => {
                assert!(
                    msg.contains("BACKEND_VENUE_MISMATCH"),
                    "error message must carry the startup failure code, got: {msg}"
                );
            }
            other => panic!("expected Some(Error(_)), got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_status_update_ready_is_none() {
        assert!(lifecycle_status_update(BackendLifecycle::Ready).is_none());
    }

    #[test]
    fn lifecycle_status_update_probing_existing_is_none() {
        assert!(lifecycle_status_update(BackendLifecycle::ProbingExisting).is_none());
    }

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
        app.init_resource::<LiveRuns>();
        app.init_resource::<SafetyToast>();
        app.init_resource::<StrategyLogs>();
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

    /// §3.10 regression: a failed SubmitSecret must surface on the SecretPrompt
    /// (so the SecretModal can show it / let the user retry), NOT on OrderFeedback
    /// (which would pop the OrderPanel out of context).
    #[test]
    fn secret_submit_failed_sets_secret_prompt_error_not_order_feedback() {
        let mut status = BackendStatus::default();
        let mut last_run = LastRunResult::default();
        let mut portfolio = PortfolioState::default();
        let mut available = AvailableInstruments::default();
        let mut progress = ReplayStartupProgress::default();
        let mut venue_status = VenueStatusRes::default();
        let mut exec_mode = ExecutionModeRes::default();
        let mut tickers = Tickers::default();
        let mut last_prices = LastPrices::default();
        let mut live_orders = LiveOrders::default();
        let mut order_feedback = OrderFeedback::default();
        let mut reconcile_prompt = ReconcilePrompt::default();
        let mut secret_prompt = SecretPrompt::default();
        let mut promote_feedback = PromoteFeedback::default();

        apply_status_update(
            BackendStatusUpdate::SecretSubmitFailed {
                error_code: "SECOND_SECRET_INVALID".to_string(),
            },
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
            &mut reconcile_prompt,
            &mut secret_prompt,
            &mut promote_feedback,
        );

        assert!(
            secret_prompt
                .error
                .as_deref()
                .is_some_and(|e| e.contains("SECOND_SECRET_INVALID")),
            "secret-flow failure must attach its error to the SecretPrompt"
        );
        assert!(
            order_feedback.message.is_none(),
            "secret-flow failure must NOT pollute the OrderPanel feedback line"
        );
    }

    fn drain_app() -> (App, mpsc::UnboundedSender<BackendEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut app = App::new();
        app.insert_resource(BackendEventChannel { rx });
        app.init_resource::<SecretPrompt>();
        app.init_resource::<LiveOrders>();
        app.init_resource::<PortfolioState>();
        app.init_resource::<ReloginPrompt>();
        app.init_resource::<LiveRuns>();
        app.init_resource::<SafetyToast>();
        app.init_resource::<StrategyLogs>();
        app.add_systems(Update, backend_event_drain_system);
        (app, tx)
    }

    /// §2.8 / §2.9: a LiveStrategyTelemetry push drives the LiveRuns counters.
    #[test]
    fn telemetry_push_drives_live_runs_counters() {
        let (mut app, tx) = drain_app();
        tx.send(BackendEvent::LiveStrategyTelemetry {
            run_id: "run-1".to_string(),
            strategy_id: "LIVE-abc12345".to_string(),
            realized_pnl: 5000.0,
            unrealized_pnl: 1234.0,
            order_count: 4,
            fill_count: 2,
            ts_ms: 100,
        })
        .unwrap();
        app.update();
        let runs = app.world().resource::<crate::trading::LiveRuns>();
        assert_eq!(
            runs.runs.len(),
            1,
            "telemetry upserts a run even before lifecycle"
        );
        let r = &runs.runs[0];
        assert_eq!(r.realized_pnl, 5000.0);
        assert_eq!(r.unrealized_pnl, 1234.0);
        assert_eq!(r.order_count, 4);
        assert_eq!(r.fill_count, 2);
        assert_eq!(r.strategy_id, "LIVE-abc12345");
    }

    /// §2.9 merge invariant through the drain: an OrderEvent's non-empty strategy_id
    /// tags the row; a later empty EC-stream OrderEvent must not clear it.
    #[test]
    fn order_event_strategy_id_merges_through_drain() {
        let (mut app, tx) = drain_app();
        tx.send(BackendEvent::OrderEvent {
            order_id: "o1".to_string(),
            venue_order_id: "v1".to_string(),
            client_order_id: "c1".to_string(),
            status: "ACCEPTED".to_string(),
            filled_qty: 0.0,
            avg_price: 0.0,
            ts_ms: 10,
            strategy_id: "LIVE-abc12345".to_string(),
        })
        .unwrap();
        tx.send(BackendEvent::OrderEvent {
            order_id: "o1".to_string(),
            venue_order_id: "v1".to_string(),
            client_order_id: "c1".to_string(),
            status: "FILLED".to_string(),
            filled_qty: 100.0,
            avg_price: 2500.0,
            ts_ms: 20,
            // untagged EC-stream follow-up.
            strategy_id: String::new(),
        })
        .unwrap();
        app.update();
        let orders = app.world().resource::<crate::trading::LiveOrders>();
        assert_eq!(orders.orders.len(), 1);
        assert_eq!(
            orders.orders[0].strategy_id, "LIVE-abc12345",
            "empty EC-stream strategy_id must not clear the tagged row"
        );
        assert_eq!(orders.orders[0].status, "FILLED");
    }
}
