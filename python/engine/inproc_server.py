"""InprocLiveServer — Phase 4 / issue #64.

Thin façade over BackendService that lets Rust call live Python methods
directly via PyO3, bypassing gRPC TCP+protobuf round-trips.

Design decisions:
- BackendService owns GrpcDataEngineServer internally (token="" so _token_ok() always passes).
- All return values are plain Python dicts so PyO3 can extract them without
  proto imports on the Rust side.
- get_state_json() delegates to BackendService.get_state_json() so live
  mode returns price-cache / depth-cache enriched state.
"""
from __future__ import annotations

import logging
from typing import Optional


class InprocLiveServer:
    """Direct-call façade over BackendService for in-process Rust dispatch."""

    def __init__(self, data_engine, live_venue_id: Optional[str] = None):
        from .backend_service import BackendService
        from .live.state_machine import VenueStateMachine
        from .mode_manager import ModeManager

        venue_sm = VenueStateMachine()

        mode_manager = getattr(data_engine, "mode_manager", None)
        if mode_manager is None:
            mode_manager = ModeManager(venue_sm=venue_sm, replay_engine=data_engine)

        factory = None
        if live_venue_id:
            try:
                from .live.live_adapter_factory import build_live_adapter_factory
                factory = build_live_adapter_factory(live_venue_id)
            except Exception:
                logging.warning(
                    "[inproc] live_adapter_factory build failed for venue_id=%r",
                    live_venue_id,
                    exc_info=True,
                )

        self._svc = BackendService(
            engine=data_engine,
            mode_manager=mode_manager,
            venue_sm=venue_sm,
            live_adapter_factory=factory,
            live_venue_id=live_venue_id,
        )

        # Slice 2: Pause/Step/Resume state for the nautilus backtest background thread.
        # set=running, clear=paused. None when no backtest is active.
        self._backtest_pause_event = None
        self._backtest_step_event = None
        self._backtest_speed_ref: list = [0.0]  # Slice 7: mutable speed cell [0.0=unlimited]

    # ------------------------------------------------------------------
    # State polling
    # ------------------------------------------------------------------

    def get_state_json(self) -> str:
        """Return JSON from BackendService.get_state_json() (includes live prices/depth)."""
        return self._svc.get_state_json()

    # ------------------------------------------------------------------
    # Venue lifecycle
    # ------------------------------------------------------------------

    def venue_login(
        self,
        venue_id: str,
        credentials_source: str,
        environment_hint: Optional[str],
    ) -> dict:
        return self._svc.venue_login(venue_id, credentials_source, environment_hint)

    def venue_logout(self) -> dict:
        return self._svc.venue_logout()

    # ------------------------------------------------------------------
    # Execution mode
    # ------------------------------------------------------------------

    def set_execution_mode(self, mode: str) -> dict:
        return self._svc.set_execution_mode(mode)

    # ------------------------------------------------------------------
    # Instruments
    # ------------------------------------------------------------------

    def list_instruments(self, source: str) -> dict:
        return self._svc.list_instruments(source)

    def list_all_listed_symbols(self, end_date: str) -> dict:
        return self._svc.list_all_listed_symbols(end_date)

    # ------------------------------------------------------------------
    # Market data subscriptions
    # ------------------------------------------------------------------

    def subscribe_market_data(self, instrument_id: str) -> dict:
        return self._svc.subscribe_market_data(instrument_id)

    def unsubscribe_market_data(self, instrument_id: str) -> dict:
        return self._svc.unsubscribe_market_data(instrument_id)

    # ------------------------------------------------------------------
    # Orders
    # ------------------------------------------------------------------

    def place_order(
        self,
        venue: str,
        instrument_id: str,
        side: str,
        qty: float,
        price: Optional[float],
        order_type: str,
        time_in_force: str,
        second_secret: Optional[str],
    ) -> dict:
        return self._svc.place_order(
            venue=venue,
            instrument_id=instrument_id,
            side=side,
            qty=qty,
            price=price,
            order_type=order_type,
            time_in_force=time_in_force,
            second_secret=second_secret,
        )

    def cancel_order(
        self,
        venue: str,
        order_id: str,
        second_secret: Optional[str],
    ) -> dict:
        return self._svc.cancel_order(venue=venue, order_id=order_id, second_secret=second_secret)

    def modify_order(
        self,
        venue: str,
        client_order_id: str,
        new_qty: Optional[float],
        new_price: Optional[float],
        second_secret: Optional[str],
    ) -> dict:
        return self._svc.modify_order(
            venue=venue,
            client_order_id=client_order_id,
            new_qty=new_qty,
            new_price=new_price,
            second_secret=second_secret,
        )

    def get_orders(self, venue: str) -> dict:
        return self._svc.get_orders(venue)

    def submit_secret(self, request_id: str, secret: str) -> dict:
        return self._svc.submit_secret(request_id, secret)

    def force_account_snapshot(self) -> dict:
        return self._svc.force_account_snapshot()

    # ------------------------------------------------------------------
    # Live strategy lifecycle
    # ------------------------------------------------------------------

    def register_live_strategy(self, strategy_file: str) -> dict:
        return self._svc.register_live_strategy(strategy_file)

    def start_live_strategy(
        self,
        strategy_id: str,
        instrument_id: str,
        venue: str,
        safety_limits_dict: Optional[dict] = None,
    ) -> dict:
        return self._svc.start_live_strategy(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            venue=venue,
            safety_limits_dict=safety_limits_dict,
        )

    def stop_live_strategy(self, run_id: str) -> dict:
        return self._svc.stop_live_strategy(run_id)

    def pause_live_strategy(self, run_id: str) -> dict:
        return self._svc.pause_live_strategy(run_id)

    def resume_live_strategy(self, run_id: str) -> dict:
        return self._svc.resume_live_strategy(run_id)

    # ------------------------------------------------------------------
    # Strategy engine run (used by RunStrategy command)
    # ------------------------------------------------------------------

    def start_engine(self, cfg: dict) -> dict:
        """Delegate to BackendService.start_engine() for strategy backtest runs."""
        return self._svc.start_engine(cfg)

    def get_portfolio(self) -> dict:
        return self._svc.get_portfolio()

    # ------------------------------------------------------------------
    # Nautilus BacktestEngine replay (issue #68 Slice 1)
    # ------------------------------------------------------------------

    def start_nautilus_replay(self, cfg: dict) -> dict:
        """Run a strategy via nautilus BacktestEngine and stream bars to RustBacktestSink.

        Slice 2: runs the backtest on a daemon background thread so the Python worker
        thread remains free to process Pause/Step/Resume commands concurrently.
        Returns immediately once the thread starts; completion is signalled via
        rust_sink.push_run_complete() or rust_sink.push_run_failed().

        cfg keys:
            strategy_file   str   — path to strategy .py
            instruments     list  — e.g. ["1301.TSE"]
            start_date      str   — "YYYY-MM-DD"
            end_date        str   — "YYYY-MM-DD"
            granularity     str   — "Daily" | "Minute"
            initial_cash    float — optional, default 10_000_000
            catalog_path    str   — parquet catalog root
            rust_sink       obj   — RustBacktestSink PyO3 object

        Returns {"success": bool, "error_code": str, "error_message": str, "run_id": str}.
        """
        import threading
        from .nautilus_backtest_runner import NautilusBacktestRunner

        strategy_file = cfg.get("strategy_file", "")
        if not strategy_file:
            return {"success": False, "error_code": "NO_STRATEGY", "error_message": "strategy_file is required", "run_id": ""}

        instruments = list(cfg.get("instruments") or [])
        if not instruments:
            return {"success": False, "error_code": "NO_INSTRUMENTS", "error_message": "instruments list is required", "run_id": ""}

        rust_sink = cfg.get("rust_sink")
        if rust_sink is None:
            return {"success": False, "error_code": "NO_SINK", "error_message": "rust_sink is required", "run_id": ""}

        catalog_path = cfg.get("catalog_path") or ""
        if not catalog_path:
            return {"success": False, "error_code": "NO_CATALOG", "error_message": "catalog_path is required", "run_id": ""}

        # Create Pause/Step/Resume control events (start in running state).
        pause_event = threading.Event()
        pause_event.set()  # running by default
        step_event = threading.Event()
        self._backtest_pause_event = pause_event
        self._backtest_step_event = step_event

        runner = NautilusBacktestRunner(
            catalog_path=catalog_path,
            strategy_file=strategy_file,
            instruments=instruments,
            start_date=cfg.get("start_date") or "",
            end_date=cfg.get("end_date") or "",
            granularity=cfg.get("granularity") or "Daily",
            initial_cash=float(cfg.get("initial_cash") or 10_000_000),
            rust_sink=rust_sink,
            pause_event=pause_event,
            step_event=step_event,
            speed_ref=self._backtest_speed_ref,
        )

        def _run() -> None:
            try:
                result = runner.run()
                # runner.run() calls rust_sink.push_run_complete() on success.
                if not result["success"]:
                    try:
                        rust_sink.push_run_failed(result.get("error", "unknown error"))
                    except Exception:
                        logging.exception("[inproc] push_run_failed failed")
            except Exception:
                logging.exception("[inproc] backtest thread uncaught exception")
                try:
                    rust_sink.push_run_failed("backtest thread error")
                except Exception:
                    pass
            finally:
                # Use identity check so a concurrent second run's events are not nullified.
                if self._backtest_pause_event is pause_event:
                    self._backtest_pause_event = None
                if self._backtest_step_event is step_event:
                    self._backtest_step_event = None

        t = threading.Thread(target=_run, daemon=True, name="backtest-runner")
        t.start()
        return {"success": True, "error_code": "", "error_message": "", "run_id": ""}

    # ------------------------------------------------------------------
    # Nautilus backtest Pause / Step / Resume control (Slice 2)
    # ------------------------------------------------------------------

    def pause_backtest(self) -> dict:
        """Pause the running backtest by clearing the pause event."""
        if self._backtest_pause_event is not None:
            self._backtest_pause_event.clear()
        return {"success": True}

    def resume_backtest(self) -> dict:
        """Resume a paused backtest by setting the pause event.

        Also clears any pending step token so it is not silently consumed on the
        first bar after the next Pause (stale token from step_backtest while running).
        """
        if self._backtest_step_event is not None:
            self._backtest_step_event.clear()
        if self._backtest_pause_event is not None:
            self._backtest_pause_event.set()
        return {"success": True}

    def step_backtest(self) -> dict:
        """Advance the paused backtest by exactly one bar."""
        if self._backtest_step_event is not None:
            self._backtest_step_event.set()
        return {"success": True}

    def set_replay_speed(self, multiplier: int) -> dict:
        """Set replay speed for the running nautilus backtest (Slice 7).

        multiplier 0  = unlimited (no delay between bars)
        multiplier 1  = 1x speed (BASE_DELAY_S per bar)
        multiplier N  = N x speed (BASE_DELAY_S / N per bar)
        """
        self._backtest_speed_ref[0] = float(multiplier)
        return {"success": True}

    def close(self) -> None:
        """Tear down the underlying live server (loop/runner/account-sync).

        Phase 4 / issue #64 finding #6: the InProc worker drops this façade
        when its command channel closes, but the wrapped BackendService's
        live loop thread + runner/account-sync survive. close() must stop them.
        """
        try:
            self._svc.teardown()
        except Exception:
            logging.exception("[inproc] close: teardown failed")
        try:
            self._svc.stop_live_loop(timeout=1.0)
        except Exception:
            logging.exception("[inproc] close: stop_live_loop failed")


def _parse_granularity_int(granularity) -> int:
    """Coerce granularity (proto enum int OR name string) to ReplayGranularity int.

    Rust backend_transport.rs passes the proto enum int directly
    (TICK=0, SECOND=1, MINUTE=2, DAILY=3), while legacy callers may pass
    the name string ('Daily'/'Minute'). Unknown values fall back to TICK(0).
    """
    from .proto import engine_pb2
    # bool is an int subclass (True == 1); reject before the int branch.
    if isinstance(granularity, bool):
        return engine_pb2.TICK
    if isinstance(granularity, int):
        if engine_pb2.TICK <= granularity <= engine_pb2.DAILY:
            return granularity
        return engine_pb2.TICK
    if granularity == "Daily":
        return engine_pb2.DAILY
    if granularity in ("Minute", "MINUTE"):
        return engine_pb2.MINUTE
    return engine_pb2.TICK
