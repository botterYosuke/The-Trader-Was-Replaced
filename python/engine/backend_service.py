"""BackendService — issue #68 Slice 11 (完全実装).

BackendService が GrpcDataEngineServer を内部で生成・隠蔽する。
InprocLiveServer はこのクラスだけを import すればよい。
"""
from __future__ import annotations

import logging
from typing import Optional


class _NullContext:
    """Minimal gRPC context stub for inproc direct-call dispatch."""

    def abort(self, code, details):
        raise RuntimeError(f"[backend_service] abort {code}: {details}")

    def is_active(self):
        return True


def _proto_order_event_to_dict(ev) -> dict:
    """Convert engine_pb2.OrderEvent → plain dict for Rust extraction."""
    d = {
        "order_id": ev.order_id,
        "venue_order_id": ev.venue_order_id,
        "client_order_id": ev.client_order_id,
        "status": ev.status,
        "filled_qty": ev.filled_qty,
        "avg_price": ev.avg_price,
        "ts_ms": ev.ts_ms,
        "strategy_id": ev.strategy_id,
        "symbol": ev.symbol,
        "side": ev.side,
        "qty": ev.qty,
    }
    if ev.HasField("price"):
        d["price"] = ev.price
    else:
        d["price"] = None
    return d


def _parse_granularity_int(granularity) -> int:
    """Coerce granularity (proto enum int OR name string) to ReplayGranularity int."""
    from .proto import engine_pb2

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


class BackendService:
    """GrpcDataEngineServer を包む薄いラッパー。proto 非依存の plain dict を返す。"""

    def __init__(
        self,
        engine,
        mode_manager=None,
        venue_sm=None,
        live_adapter_factory=None,
        live_venue_id=None,
        engine_controller=None,
    ) -> None:
        from .server_grpc import GrpcDataEngineServer

        self._srv = GrpcDataEngineServer(
            token="",
            engine=engine,
            mode_manager=mode_manager,
            venue_sm=venue_sm,
            live_adapter_factory=live_adapter_factory,
            live_venue_id=live_venue_id,
        )
        self._ctx = _NullContext()

    # ------------------------------------------------------------------
    # State
    # ------------------------------------------------------------------

    def get_state_json(self) -> str:
        from .proto import engine_pb2

        try:
            req = engine_pb2.GetStateRequest(token="")
            resp = self._srv.GetState(req, self._ctx)
            return resp.json_data
        except Exception:
            logging.exception("[backend_service] get_state_json failed; falling back")
            return self._srv.engine.get_current_state().model_dump_json()

    def get_portfolio(self) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.GetPortfolioRequest(token="")
        try:
            resp = self._srv.GetPortfolio(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "buying_power": 0.0, "cash": 0.0, "equity": 0.0, "positions": [], "orders": [], "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "buying_power": 0.0, "cash": 0.0, "equity": 0.0, "positions": [], "orders": [], "detail": str(exc)}
        return {
            "success": resp.success,
            "buying_power": resp.buying_power,
            "cash": resp.cash,
            "equity": resp.equity,
            "positions": [
                {"symbol": p.symbol, "qty": p.qty, "avg_price": p.avg_price, "unrealized_pnl": p.unrealized_pnl}
                for p in resp.positions
            ],
            "orders": [
                {"symbol": o.symbol, "side": o.side, "qty": o.qty, "price": o.price, "status": o.status, "ts_ms": o.ts_ms}
                for o in resp.orders
            ],
        }

    # ------------------------------------------------------------------
    # Venue lifecycle
    # ------------------------------------------------------------------

    def venue_login(
        self,
        venue_id: str,
        credentials_source: str,
        environment_hint: Optional[str],
    ) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.VenueLoginRequest(
            venue_id=venue_id,
            credentials_source=credentials_source or "prompt",
            environment_hint=environment_hint or "",
            token="",
        )
        try:
            resp = self._srv.VenueLogin(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "venue_state": "", "instruments_loaded": 0, "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "venue_state": "", "instruments_loaded": 0, "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "venue_state": resp.venue_state,
            "instruments_loaded": resp.instruments_loaded,
        }

    def venue_logout(self) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.VenueLogoutRequest(token="")
        try:
            resp = self._srv.VenueLogout(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    # ------------------------------------------------------------------
    # Execution mode
    # ------------------------------------------------------------------

    def set_execution_mode(self, mode: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.SetExecutionModeRequest(mode=mode, token="")
        try:
            resp = self._srv.SetExecutionMode(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "execution_mode": "", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "execution_mode": "", "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "execution_mode": resp.execution_mode,
        }

    # ------------------------------------------------------------------
    # Instruments
    # ------------------------------------------------------------------

    def list_instruments(self, source: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.ListInstrumentsRequest(source=source, token="")
        try:
            resp = self._srv.ListInstruments(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "instruments": [], "instrument_ids": [], "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "instruments": [], "instrument_ids": [], "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_message,
            "instrument_ids": list(resp.instrument_ids),
            "instruments": [
                {"id": i.id, "name": i.name, "market": i.market}
                for i in resp.instruments
            ],
        }

    def list_all_listed_symbols(self, end_date: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.ListAllListedSymbolsRequest(end_date=end_date, token="")
        try:
            resp = self._srv.ListAllListedSymbols(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "instrument_ids": [], "resolved_end_date": end_date, "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "instrument_ids": [], "resolved_end_date": end_date, "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_message,
            "instrument_ids": list(resp.instrument_ids),
            "resolved_end_date": resp.resolved_end_date,
        }

    # ------------------------------------------------------------------
    # Market data subscriptions
    # ------------------------------------------------------------------

    def subscribe_market_data(self, instrument_id: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.SubscribeRequest(
            instrument_id=instrument_id,
            channels=["trades", "depth"],
            token="",
        )
        try:
            resp = self._srv.SubscribeMarketData(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    def unsubscribe_market_data(self, instrument_id: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.UnsubscribeRequest(instrument_id=instrument_id, token="")
        try:
            resp = self._srv.UnsubscribeMarketData(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

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
        from .proto import engine_pb2

        req = engine_pb2.PlaceOrderReq(
            token="",
            venue=venue,
            instrument_id=instrument_id,
            side=side,
            qty=qty,
            order_type=order_type,
            time_in_force=time_in_force,
        )
        if price is not None:
            req.price = price
        if second_secret is not None:
            req.second_secret = second_secret
        try:
            resp = self._srv.PlaceOrder(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "order_event": None, "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "order_event": None, "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "order_event": _proto_order_event_to_dict(resp.order_event) if resp.HasField("order_event") else None,
        }

    def cancel_order(
        self,
        venue: str,
        order_id: str,
        second_secret: Optional[str],
    ) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.CancelOrderReq(token="", venue=venue, order_id=order_id)
        if second_secret is not None:
            req.second_secret = second_secret
        try:
            resp = self._srv.CancelOrder(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "order_event": None, "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "order_event": None, "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "order_event": _proto_order_event_to_dict(resp.order_event) if resp.HasField("order_event") else None,
        }

    def modify_order(
        self,
        venue: str,
        client_order_id: str,
        new_qty: Optional[float],
        new_price: Optional[float],
        second_secret: Optional[str],
    ) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.ModifyOrderReq(
            token="",
            venue=venue,
            order_id=client_order_id,
        )
        if new_qty is not None:
            req.new_qty = new_qty
        if new_price is not None:
            req.new_price = new_price
        if second_secret is not None:
            req.second_secret = second_secret
        try:
            resp = self._srv.ModifyOrder(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "order_event": None, "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "order_event": None, "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "order_event": _proto_order_event_to_dict(resp.order_event) if resp.HasField("order_event") else None,
        }

    def get_orders(self, venue: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.GetOrdersReq(token="", venue=venue)
        try:
            resp = self._srv.GetOrders(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "orders": [], "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "orders": [], "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "orders": [_proto_order_event_to_dict(o) for o in resp.orders],
        }

    def submit_secret(self, request_id: str, secret: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.SubmitSecretReq(token="", request_id=request_id, secret=secret)
        try:
            resp = self._srv.SubmitSecret(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    def force_account_snapshot(self) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.ForceAccountSnapshotRequest(token="")
        try:
            resp = self._srv.ForceAccountSnapshot(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    # ------------------------------------------------------------------
    # Live strategy lifecycle
    # ------------------------------------------------------------------

    def register_live_strategy(self, strategy_file: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.RegisterLiveStrategyReq(
            token="",
            request_id="",
            strategy_file=strategy_file,
            expected_sha256="",
        )
        try:
            resp = self._srv.RegisterLiveStrategy(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "strategy_id": "", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "strategy_id": "", "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "strategy_id": resp.strategy_id,
            "error_message": resp.error_message if not resp.success else "",
        }

    def start_live_strategy(
        self,
        strategy_id: str,
        instrument_id: str,
        venue: str,
        safety_limits_dict: Optional[dict] = None,
    ) -> dict:
        from .proto import engine_pb2

        safety_limits = engine_pb2.SafetyLimits()
        if safety_limits_dict:
            if "max_position_size_jpy" in safety_limits_dict:
                safety_limits.max_position_size_jpy = safety_limits_dict["max_position_size_jpy"]
            if "max_order_value_jpy" in safety_limits_dict:
                safety_limits.max_order_value_jpy = safety_limits_dict["max_order_value_jpy"]
            if "max_daily_loss_jpy" in safety_limits_dict:
                safety_limits.max_daily_loss_jpy = safety_limits_dict["max_daily_loss_jpy"]
            if "max_orders_per_minute" in safety_limits_dict:
                safety_limits.max_orders_per_minute = safety_limits_dict["max_orders_per_minute"]
            if "allowed_instruments" in safety_limits_dict:
                safety_limits.allowed_instruments.extend(safety_limits_dict["allowed_instruments"])

        req = engine_pb2.StartLiveStrategyReq(
            token="",
            request_id="",
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            venue=venue,
            safety_limits=safety_limits,
        )
        try:
            resp = self._srv.StartLiveStrategy(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "run_id": "", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "run_id": "", "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code,
            "run_id": resp.run_id if resp.success else "",
            "error_message": resp.error_message if not resp.success else "",
        }

    def stop_live_strategy(self, run_id: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.StopLiveStrategyReq(token="", request_id="", run_id=run_id)
        try:
            resp = self._srv.StopLiveStrategy(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    def pause_live_strategy(self, run_id: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.PauseLiveStrategyReq(token="", request_id="", run_id=run_id)
        try:
            resp = self._srv.PauseLiveStrategy(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    def resume_live_strategy(self, run_id: str) -> dict:
        from .proto import engine_pb2

        req = engine_pb2.ResumeLiveStrategyReq(token="", request_id="", run_id=run_id)
        try:
            resp = self._srv.ResumeLiveStrategy(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "detail": str(exc)}
        return {"success": resp.success, "error_code": resp.error_code}

    # ------------------------------------------------------------------
    # Strategy engine run (used by RunStrategy command)
    # ------------------------------------------------------------------

    def start_engine(self, cfg: dict) -> dict:
        """Delegate to GrpcDataEngineServer.StartEngine() for strategy backtest runs."""
        from .proto import engine_pb2

        engine_start_config = engine_pb2.EngineStartConfig(
            instrument_id=cfg.get("instrument_id", ""),
            instrument_ids=cfg.get("instrument_ids", []),
            strategy_file=cfg.get("strategy_file", ""),
        )
        start_date = cfg.get("start_date")
        end_date = cfg.get("end_date")
        if start_date:
            engine_start_config.start_date = start_date
        if end_date:
            engine_start_config.end_date = end_date
        if cfg.get("initial_cash") is not None:
            engine_start_config.initial_cash = str(cfg["initial_cash"])
        if cfg.get("granularity"):
            gran_int = _parse_granularity_int(cfg["granularity"])
            gran_name = self._srv._replay_granularity_name(gran_int)
            if gran_name:
                engine_start_config.granularity = gran_int

        req = engine_pb2.StartEngineRequest(
            token="",
            request_id="",
            engine=engine_pb2.NAUTILUS,
            strategy_id="",
            config=engine_start_config,
        )
        try:
            resp = self._srv.StartEngine(req, self._ctx)
        except RuntimeError as exc:
            return {"success": False, "error_code": "INPROC_ABORT", "run_id": "", "summary_json": "", "detail": str(exc)}
        except Exception as exc:
            return {"success": False, "error_code": "INPROC_ERROR", "run_id": "", "summary_json": "", "detail": str(exc)}
        return {
            "success": resp.success,
            "error_code": resp.error_code if not resp.success else "",
            "error_message": resp.error_message if not resp.success else "",
            "run_id": resp.run_id if resp.success else "",
            "summary_json": resp.summary_json if resp.success else "",
        }

    # ------------------------------------------------------------------
    # Teardown
    # ------------------------------------------------------------------

    def teardown(self) -> None:
        try:
            self._srv._teardown_live_components()
        except Exception:
            logging.exception("[backend_service] teardown: _teardown_live_components failed")
        try:
            self._srv.stop_live_loop(timeout=1.0)
        except Exception:
            logging.exception("[backend_service] teardown: stop_live_loop failed")

    def stop_live_loop(self, timeout: float = 5.0) -> None:
        try:
            self._srv.stop_live_loop(timeout=timeout)
        except Exception:
            logging.exception("[backend_service] stop_live_loop failed")
