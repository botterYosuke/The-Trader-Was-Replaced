import json
import logging
import os
import re
import threading
import time
from concurrent import futures
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

import grpc

from .core import DataEngine
from .live.state_machine import VenueStateMachine
from .mode_manager import ModeManager
from .proto import engine_pb2, engine_pb2_grpc
from .replay import BaseReplayProvider
from .jquants_loader import JQuantsLoader
from .paths import listed_symbols_artifact_path


_INSTRUMENT_ID_RE = re.compile(r"^(.+?)-\d+-[A-Z]")


def _artifact_path_for(end_date: str) -> Path:
    return listed_symbols_artifact_path(end_date)


def _read_artifact(end_date: str) -> Optional[list[str]]:
    path = _artifact_path_for(end_date)
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        logging.warning("ListAllListedSymbols: artifact read failed: %s", exc)
        return None
    if not isinstance(data, dict):
        return None
    if data.get("schema_version") != 1:
        return None
    if data.get("end_date") != end_date:
        return None
    ids = data.get("instrument_ids")
    if not isinstance(ids, list) or not all(isinstance(x, str) for x in ids):
        return None
    return ids


def _write_artifact_atomic(end_date: str, instrument_ids: list[str], catalog_path: Optional[str]) -> None:
    path = _artifact_path_for(end_date)
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "end_date": end_date,
        "source": "nautilus_catalog",
        "catalog_path": str(catalog_path) if catalog_path else "",
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "instrument_ids": instrument_ids,
    }
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    os.replace(tmp, path)


def _resolve_date_bounds_from_catalog(catalog_path: str) -> Optional[tuple[str, str]]:
    """Return (oldest_date, latest_date) as 'YYYY-MM-DD' from catalog parquet stats."""
    bar_dir = Path(catalog_path) / "data" / "bar"
    if not bar_dir.exists():
        return None
    oldest_ns: Optional[int] = None
    latest_ns: Optional[int] = None
    try:
        import pyarrow.parquet as pq
        for entry in bar_dir.iterdir():
            if not entry.is_dir() or entry.name == "backup":
                continue
            for pq_file in entry.glob("*.parquet"):
                try:
                    meta = pq.read_metadata(str(pq_file))
                    schema = meta.schema
                    for i in range(meta.num_row_groups):
                        rg = meta.row_group(i)
                        for c in range(rg.num_columns):
                            col = rg.column(c)
                            name = schema.column(c).name
                            if name in ("ts_event", "ts_init") and col.statistics is not None:
                                mn = col.statistics.min
                                mx = col.statistics.max
                                if isinstance(mn, int):
                                    if oldest_ns is None or mn < oldest_ns:
                                        oldest_ns = mn
                                if isinstance(mx, int):
                                    if latest_ns is None or mx > latest_ns:
                                        latest_ns = mx
                except Exception:
                    continue
    except Exception as exc:
        logging.warning("ListAllListedSymbols: catalog scan stats failed: %s", exc)
    if oldest_ns is None or latest_ns is None or latest_ns <= 0:
        return None

    def _to_date(ns: int) -> str:
        secs = ns / 1_000_000_000
        return datetime.fromtimestamp(secs, tz=timezone.utc).strftime("%Y-%m-%d")

    return _to_date(oldest_ns), _to_date(latest_ns)


def _resolve_latest_end_date_from_catalog(catalog_path: str) -> Optional[str]:
    bounds = _resolve_date_bounds_from_catalog(catalog_path)
    return bounds[1] if bounds else None


def _scan_catalog_instruments(catalog_path: str) -> list[str]:
    bar_dir = Path(catalog_path) / "data" / "bar"
    if not bar_dir.exists():
        return []
    seen: set[str] = set()
    for entry in bar_dir.iterdir():
        if not entry.is_dir() or entry.name == "backup":
            continue
        m = _INSTRUMENT_ID_RE.match(entry.name)
        if m:
            seen.add(m.group(1))
    return sorted(seen)


class GrpcDataEngineServer(
    engine_pb2_grpc.HealthServicer, engine_pb2_grpc.DataEngineServicer
):
    def __init__(
        self,
        token: str,
        engine: DataEngine,
        mode_manager=None,
        venue_sm=None,
    ):
        self.token = token
        self.engine = engine
        self.mode_manager = mode_manager
        self.venue_sm = venue_sm

    _KNOWN_VENUES = {"TACHIBANA", "KABU"}
    _KNOWN_CRED_SOURCES = {"prompt", "session_cache", "env"}
    _KNOWN_MODES = {"Replay", "LiveManual", "LiveAuto"}

    def _current_engine_state(self):
        """Map the core replay state string to the gRPC EngineState enum."""
        state = self.engine.replay_state
        if state == "IDLE":
            return engine_pb2.IDLE
        if state == "LOADED":
            return engine_pb2.LOADED
        if state == "RUNNING":
            return engine_pb2.RUNNING
        if state == "PAUSED":
            return engine_pb2.PAUSED
        if state == "STOPPING":
            return engine_pb2.STOPPING
        return engine_pb2.IDLE

    def _replay_granularity_name(self, granularity):
        if granularity == engine_pb2.TICK:
            return "Trade"
        if granularity == engine_pb2.MINUTE:
            return "Minute"
        if granularity == engine_pb2.DAILY:
            return "Daily"
        return None

    def Check(self, request, context):
        return engine_pb2.HealthCheckResponse(
            status=engine_pb2.HealthCheckResponse.SERVING
        )

    def GetState(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        state = self.engine.get_current_state()
        return engine_pb2.GetStateResponse(json_data=state.model_dump_json())

    def Start(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        self.engine.start()
        logging.info("Engine start requested via gRPC")
        return engine_pb2.StartResponse(success=True)

    def Stop(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        self.engine.stop()
        logging.info("Engine stop requested via gRPC")
        return engine_pb2.StopResponse(success=True)

    def LoadReplayData(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        granularity_name = self._replay_granularity_name(request.granularity)
        if granularity_name is None:
            return engine_pb2.ReplayControlResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="INVALID_STATE",
                error_message=f"Granularity {request.granularity} is not supported",
            )

        catalog_path = request.catalog_path if request.HasField("catalog_path") else None
        success, error = self.engine.load_replay_data(
            request.instrument_ids,
            request.start_date,
            request.end_date,
            granularity_name,
            catalog_path=catalog_path,
        )
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def StartEngine(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        strategy_file = request.config.strategy_file if request.HasField("config") and request.config.HasField("strategy_file") else None
        logging.info(f"StartEngine: strategy_file={strategy_file!r}")

        if not strategy_file:
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="MISSING_STRATEGY_FILE",
                error_message="StartEngine requires config.strategy_file",
            )

        try:
            from engine.strategy_runtime.strategy_loader import load as _load_strategy, StrategyLoadError
            _module, scenario, strategy_cls = _load_strategy(strategy_file)
            logging.info(
                f"StartEngine: strategy loaded cls={strategy_cls.__name__!r}"
                f" instruments={scenario.get('instruments') or [scenario.get('instrument')]}"
                f" granularity={scenario.get('granularity')!r}"
                f" start={scenario.get('start')!r} end={scenario.get('end')!r}"
            )
        except FileNotFoundError as exc:
            logging.error(f"StartEngine: strategy file not found: {exc}")
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="STRATEGY_FILE_NOT_FOUND",
                error_message=str(exc),
            )
        except Exception as exc:
            logging.error(f"StartEngine: strategy load failed: {exc}")
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="STRATEGY_LOAD_ERROR",
                error_message=str(exc),
            )

        catalog_path = self.engine.last_replay_catalog_path
        if not catalog_path:
            logging.error("StartEngine: catalog_path not available (LoadReplayData not called or no catalog configured)")
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="CATALOG_PATH_NOT_AVAILABLE",
                error_message="No catalog_path available from prior LoadReplayData",
            )

        try:
            from engine.strategy_runtime.catalog_data_loader import load_bars_for_scenario
            bars_by_instrument = load_bars_for_scenario(catalog_path, scenario)
            total_bars = sum(len(v) for v in bars_by_instrument.values())
            per_instrument = {str(k): len(v) for k, v in bars_by_instrument.items()}
            logging.info(
                f"StartEngine: bars loaded total={total_bars} per_instrument={per_instrument}"
            )
        except Exception as exc:
            logging.error(f"StartEngine: catalog bars load failed: {exc}")
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="CATALOG_BARS_LOAD_ERROR",
                error_message=str(exc),
            )

        import json as _json
        # Transition LOADED → RUNNING before engine_run so PauseReplay can work mid-run.
        se_ok, se_err = self.engine.start_engine()
        if not se_ok:
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="INVALID_STATE",
                error_message=se_err or "",
            )

        try:
            from engine.strategy_runtime.run_buffer import (
                RunBuffer,
                make_run_id,
                get_run_buffer_base_dir,
            )
            from engine.strategy_runtime.engine_runner import run as engine_run
            from engine.strategy_runtime.summary import compute_summary, write_summary_json

            instruments = scenario.get("instruments") or [scenario.get("instrument", "unknown")]
            first_instrument = instruments[0] if instruments else "unknown"
            run_id = make_run_id(strategy_file, first_instrument)

            rb = RunBuffer(
                run_id=run_id,
                strategy_file=str(strategy_file),
                scenario=scenario,
                base_dir=get_run_buffer_base_dir(),
            )

            try:
                engine_run(
                    strategy_cls=strategy_cls,
                    scenario=scenario,
                    bars_by_instrument=bars_by_instrument,
                    run_buffer=rb,
                    strategy_init_kwargs=None,
                    run_event=self.engine.run_event,
                    bar_interval_sec=0.01,
                )
                rb.finish()

                # Expose ALL bars to GetState so the chart can draw multiple candles.
                # bars[0] was already primed by _prime_provider_locked; inject bars[1:].
                from .nautilus_adapter import bar_to_kline_update
                for bars in bars_by_instrument.values():
                    if bars:
                        for bar in bars[1:]:
                            self.engine.apply_replay_event(bar_to_kline_update(bar))
                    break

                summary = compute_summary(rb.run_dir)
                write_summary_json(rb.run_dir, summary)

                from engine.strategy_runtime.portfolio import compute_portfolio
                self.engine.last_portfolio = compute_portfolio(rb.run_dir, scenario)

                logging.info(
                    "StartEngine: run complete run_id=%s run_dir=%s summary=%r",
                    run_id,
                    rb.run_dir,
                    summary,
                )
            except Exception as exc:
                rb.abort()
                self.engine.force_stop_replay()
                logging.exception("StartEngine: engine_runner failed")
                return engine_pb2.StartEngineResponse(
                    success=False,
                    request_id=request.request_id,
                    current_state=self._current_engine_state(),
                    error_code="RUN_FAILED",
                    error_message=str(exc),
                )
        except ImportError as exc:
            self.engine.force_stop_replay()
            logging.error("StartEngine: RunBuffer/engine_runner import failed: %s", exc)
            return engine_pb2.StartEngineResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="RUN_FAILED",
                error_message=str(exc),
            )

        self.engine.force_stop_replay()
        resp = engine_pb2.StartEngineResponse(
            success=True,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
        )
        resp.run_id = run_id
        resp.summary_json = _json.dumps(summary, ensure_ascii=False)
        return resp

    def StopEngine(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.stop_replay()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def PauseReplay(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.pause_replay()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def ResumeReplay(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.resume_replay()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def StepReplay(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.step_replay()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def StopReplay(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.stop_replay()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def ForceStopReplay(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.force_stop_replay()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def SetReplaySpeed(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        success, error = self.engine.set_replay_speed(request.multiplier)
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

    def GetPortfolio(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        p = self.engine.last_portfolio
        if p is None:
            return engine_pb2.GetPortfolioResponse(success=True)

        positions = [
            engine_pb2.PortfolioPosition(
                symbol=pos.get("symbol", ""),
                qty=int(pos.get("qty", 0)),
                avg_price=float(pos.get("avg_price", 0.0)),
                unrealized_pnl=float(pos.get("unrealized_pnl", 0.0)),
            )
            for pos in p.get("positions", [])
        ]
        orders = [
            engine_pb2.PortfolioOrder(
                symbol=ord_.get("symbol", ""),
                side=ord_.get("side", ""),
                qty=float(ord_.get("qty", 0.0)),
                price=float(ord_.get("price", 0.0)),
                status=ord_.get("status", ""),
                ts_ms=int(ord_.get("ts_ms", 0)),
            )
            for ord_ in p.get("orders", [])
        ]
        return engine_pb2.GetPortfolioResponse(
            success=True,
            buying_power=float(p.get("buying_power", 0.0)),
            cash=float(p.get("cash", 0.0)),
            equity=float(p.get("equity", 0.0)),
            positions=positions,
            orders=orders,
        )

    def ListInstruments(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        catalog_path = self.engine.last_replay_catalog_path or self.engine._jquants_catalog_path
        if not catalog_path:
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message="No catalog_path available",
            )

        try:
            from pathlib import Path
            import re

            bar_dir = Path(catalog_path) / "data" / "bar"
            if not bar_dir.exists():
                return engine_pb2.ListInstrumentsResponse(
                    success=True,
                    instrument_ids=[],
                )

            seen: set[str] = set()
            for entry in bar_dir.iterdir():
                if not entry.is_dir() or entry.name == "backup":
                    continue
                m = re.match(r"^(.+?)-\d+-[A-Z]", entry.name)
                if m:
                    seen.add(m.group(1))

            ids = sorted(seen)
            logging.info("ListInstruments: found %d instruments: %s", len(ids), ids)
            return engine_pb2.ListInstrumentsResponse(success=True, instrument_ids=ids)
        except Exception as exc:
            logging.error("ListInstruments: error: %s", exc)
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message=str(exc),
            )

    def ListAllListedSymbols(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        end_date = (request.end_date or "").strip()
        catalog_path = (
            self.engine.last_replay_catalog_path or self.engine._jquants_catalog_path
        )

        resolved_end_date = end_date
        if not resolved_end_date:
            if catalog_path:
                resolved_end_date = _resolve_latest_end_date_from_catalog(catalog_path) or ""
            if not resolved_end_date:
                resolved_end_date = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        else:
            try:
                datetime.strptime(resolved_end_date, "%Y-%m-%d")
            except ValueError as exc:
                return engine_pb2.ListAllListedSymbolsResponse(
                    success=False,
                    error_message=f"Invalid end_date '{resolved_end_date}': {exc}",
                )

        # Fast path: if the artifact already exists for the requested end_date,
        # serve it without scanning catalog parquet metadata. The bounds-resolve
        # scan walks every per-instrument parquet (~600 files, ~40s on cold cache)
        # and is only needed to clamp out-of-range end_dates / detect before_oldest;
        # any artifact on disk was written for a valid in-range end_date, so skipping
        # the scan is safe here.
        if end_date:
            fast_cached = _read_artifact(resolved_end_date)
            if fast_cached is not None:
                logging.info(
                    "ListAllListedSymbols: artifact hit (fast path) end_date=%s count=%d",
                    resolved_end_date, len(fast_cached),
                )
                return engine_pb2.ListAllListedSymbolsResponse(
                    success=True,
                    instrument_ids=fast_cached,
                    resolved_end_date=resolved_end_date,
                )

        before_oldest = False
        if end_date and catalog_path:
            bounds = _resolve_date_bounds_from_catalog(catalog_path)
            if bounds is not None:
                oldest_date, latest_date = bounds
                if resolved_end_date > latest_date:
                    resolved_end_date = latest_date
                if resolved_end_date < oldest_date:
                    before_oldest = True

        if before_oldest:
            try:
                _write_artifact_atomic(resolved_end_date, [], catalog_path)
            except Exception as exc:
                logging.warning("ListAllListedSymbols: artifact write failed: %s", exc)
            logging.info(
                "ListAllListedSymbols: end_date=%s before catalog oldest -> empty ids",
                resolved_end_date,
            )
            return engine_pb2.ListAllListedSymbolsResponse(
                success=True,
                instrument_ids=[],
                resolved_end_date=resolved_end_date,
            )

        cached = _read_artifact(resolved_end_date)
        if cached is not None:
            logging.info("ListAllListedSymbols: artifact hit end_date=%s count=%d", resolved_end_date, len(cached))
            return engine_pb2.ListAllListedSymbolsResponse(
                success=True,
                instrument_ids=cached,
                resolved_end_date=resolved_end_date,
            )

        if not catalog_path:
            return engine_pb2.ListAllListedSymbolsResponse(
                success=False,
                error_message="No catalog_path available",
                resolved_end_date=resolved_end_date,
            )

        try:
            ids = _scan_catalog_instruments(catalog_path)
        except Exception as exc:
            logging.error("ListAllListedSymbols: scan failed: %s", exc)
            return engine_pb2.ListAllListedSymbolsResponse(
                success=False,
                error_message=str(exc),
                resolved_end_date=resolved_end_date,
            )

        ids = sorted(set(ids))

        try:
            _write_artifact_atomic(resolved_end_date, ids, catalog_path)
        except Exception as exc:
            logging.warning("ListAllListedSymbols: artifact write failed: %s", exc)

        logging.info("ListAllListedSymbols: miss->write end_date=%s count=%d", resolved_end_date, len(ids))
        return engine_pb2.ListAllListedSymbolsResponse(
            success=True,
            instrument_ids=ids,
            resolved_end_date=resolved_end_date,
        )

    def VenueLogin(self, request, context):
        if request.credentials_source not in self._KNOWN_CRED_SOURCES:
            context.abort(
                grpc.StatusCode.INVALID_ARGUMENT,
                "INVALID_CREDENTIALS_SOURCE",
            )

        venue_state = self.venue_sm.current if self.venue_sm is not None else "DISCONNECTED"

        if request.venue_id not in self._KNOWN_VENUES:
            return engine_pb2.VenueLoginResponse(
                success=False,
                error_code="UNKNOWN_VENUE",
                venue_state=venue_state,
                instruments_loaded=0,
            )

        if request.venue_id == "KABU" and request.credentials_source == "session_cache":
            return engine_pb2.VenueLoginResponse(
                success=False,
                error_code="UNSUPPORTED_FOR_VENUE",
                venue_state=venue_state,
                instruments_loaded=0,
            )

        return engine_pb2.VenueLoginResponse(
            success=False,
            error_code="NOT_IMPLEMENTED",
            venue_state=venue_state,
            instruments_loaded=0,
        )

    def VenueLogout(self, request, context):
        return engine_pb2.VenueControlResponse(success=True, error_code="")

    def SetExecutionMode(self, request, context):
        if request.mode not in self._KNOWN_MODES:
            context.abort(grpc.StatusCode.INVALID_ARGUMENT, "INVALID_MODE")

        if self.mode_manager is None:
            return engine_pb2.SetExecutionModeResponse(
                success=False,
                error_code="NOT_IMPLEMENTED",
                execution_mode="",
            )

        try:
            applied = self.mode_manager.set_execution_mode(request.mode)
        except ValueError as exc:
            msg = str(exc)
            code = "EXECUTION_MODE_PRECONDITION" if msg.startswith("EXECUTION_MODE_PRECONDITION") else "EXECUTION_MODE_ERROR"
            return engine_pb2.SetExecutionModeResponse(
                success=False,
                error_code=code,
                execution_mode="",
            )

        return engine_pb2.SetExecutionModeResponse(
            success=True,
            error_code="",
            execution_mode=applied,
        )

    def SubscribeMarketData(self, request, context):
        return engine_pb2.SubscribeResponse(success=False, error_code="NOT_IMPLEMENTED")

    def UnsubscribeMarketData(self, request, context):
        return engine_pb2.SubscribeResponse(success=False, error_code="NOT_IMPLEMENTED")


def advance_loop(engine: DataEngine, interval: float = 1.0):
    """Advance the engine on a fixed background interval while it is running."""
    logging.info(f"Starting advance loop with interval {interval}s")
    while True:
        time.sleep(interval)
        if engine.is_running:
            engine.advance()
    logging.info("Advance loop stopped")


def serve(
    port: int,
    token: str,
    replay_provider: Optional[BaseReplayProvider] = None,
    auto_start: bool = False,
    max_history_len: int = 1000,
    advance_interval_sec: float = 1.0,
    jquants_dir: Optional[str] = None,
    jquants_catalog_path: Optional[str] = None,
):
    jquants_loader = JQuantsLoader(jquants_dir) if jquants_dir else None

    venue_sm = VenueStateMachine()
    engine = DataEngine(
        replay_provider=replay_provider,
        max_history_len=max_history_len,
        jquants_loader=jquants_loader,
        jquants_catalog_path=jquants_catalog_path,
        state_machine=venue_sm,
    )
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    # Keep replay sessions paused at startup unless explicitly requested.
    if auto_start:
        engine.start()
    else:
        logging.info("Engine initialized in paused state.")

    ticker_thread = threading.Thread(
        target=advance_loop, args=(engine, advance_interval_sec), daemon=True
    )
    ticker_thread.start()

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    servicer = GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)

    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)

    server.add_insecure_port(f"127.0.0.1:{port}")
    logging.info(f"Starting gRPC server on port {port}")
    server.start()
    try:
        while True:
            time.sleep(86400)
    except KeyboardInterrupt:
        engine.stop()
        server.stop(0)
