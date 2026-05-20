import json
import logging
import os
import re
import sys
import asyncio
import tempfile
import threading
import time
import signal
from concurrent import futures
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

import grpc

from . import process_lifecycle
from .core import DataEngine
from .live._build_mode import IS_DEBUG_BUILD
from .live.live_adapter_factory import build_live_adapter_factory
from .live.live_runner import LiveRunner
from .live.reducer_bridge import LiveReducerBridge
from .live.last_price_cache import LastPriceCache
from .live.depth_cache import DepthCache
from .live.state_machine import VenueStateMachine
from .live.backend_event_bus import BackendEventBus
from .live.secret_vault import SecretVault
from .live.order_facade import ManualOrderFacade, OrderFacadeError
from .live.account_sync import AccountSync
from .mode_manager import ModeManager
from .models import PerInstrumentState
from .proto import engine_pb2, engine_pb2_grpc
from .replay import BaseReplayProvider
from .jquants_loader import JQuantsLoader
from .paths import listed_symbols_artifact_path
from engine.strategy_runtime.catalog_data_loader import load_bars_for_scenario, normalize_granularity
from engine.jquants_to_catalog import ensure_jquants_catalog


def engine_run(*args, **kwargs):
    from engine.strategy_runtime.engine_runner import run
    return run(*args, **kwargs)


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


def _sweep_stale_cred_files(max_age_s: float = 60.0) -> None:
    """Delete leftover ttwr_cred_*.json files older than ``max_age_s`` seconds."""
    try:
        tmp_dir = Path(tempfile.gettempdir())
        now = time.time()
        for stale in tmp_dir.glob("ttwr_cred_*.json"):
            try:
                if now - stale.stat().st_mtime > max_age_s:
                    stale.unlink()
            except OSError:
                continue
    except OSError:
        pass


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


_ADAPTER_ERROR_CODES = frozenset({
    "SESSION_CACHE_MISSING",
    "SESSION_CACHE_EXPIRED",
    "PROMPT_RESULT_MISSING_TOKEN",
})


def _live_login_timeout_s() -> float:
    return float(os.environ.get("LIVE_LOGIN_TIMEOUT_S", "180"))


class GrpcDataEngineServer(
    engine_pb2_grpc.HealthServicer, engine_pb2_grpc.DataEngineServicer
):
    def __init__(
        self,
        token: str,
        engine: DataEngine,
        mode_manager=None,
        venue_sm=None,
        live_adapter_factory=None,
        live_venue_id: Optional[str] = None,
    ):
        self.token = token
        self.engine = engine
        self.mode_manager = mode_manager
        self.venue_sm = venue_sm
        self._live_adapter_factory = live_adapter_factory
        self._live_runner = None
        self._live_bridge = None
        self._live_loop = None
        self._live_thread = None
        self._live_price_cache: Optional[LastPriceCache] = None
        self._live_depth_cache: Optional[DepthCache] = None
        self._live_timeout_s = 5.0
        # When True, suppress live_last_error on the next GetState. Armed on
        # Live re-enter / VenueLogin success / Replay toggle, cleared lazily by
        # the next observed error from runner/bridge.
        self._suppress_live_last_error: bool = False
        # D21: venue id from --live-venue flag, uppercase normalized
        self._live_venue_id: Optional[str] = live_venue_id.upper() if live_venue_id else None
        # Phase 9 Step 0: backend → frontend event push (threading-based fan-out).
        # Lifetime = servicer lifetime; per-stream cleanup is in the handler finally.
        self._backend_event_bus: BackendEventBus = BackendEventBus()
        # Phase 9 Step 1: secret relay vault (Tachibana second-password).
        self._secret_vault: SecretVault = SecretVault()
        # Phase 9 Step 2: manual order execution facade (lifetime = live session).
        self._order_facade: Optional[ManualOrderFacade] = None
        # Phase 9 Step 4: account sync (余力・建玉の定期 push, lifetime = live session).
        self._account_sync: Optional[AccountSync] = None

    _KNOWN_VENUES = {"TACHIBANA", "KABU", "MOCK"}  # D26: MOCK added
    _KNOWN_CRED_SOURCES = {"prompt", "session_cache", "env", "prompt_result"}
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

    def _ensure_live_loop(self):
        if self._live_loop is not None:
            return self._live_loop

        loop = asyncio.new_event_loop()

        def _loop_exception_handler(_loop, ctx):
            # Post-merge fix (MEDIUM-5): mask any secrets that may have ended up
            # in the asyncio context (exception args / message) before logging.
            try:
                from engine.live.logging import mask_secrets
                masked = mask_secrets({k: str(v) for k, v in ctx.items()})
            except Exception:
                masked = {"message": "<context masking failed>"}
            logging.error("phase8-live-loop uncaught asyncio exception: %s", masked)

        def run_loop():
            asyncio.set_event_loop(loop)
            loop.set_exception_handler(_loop_exception_handler)
            try:
                loop.run_forever()
            except BaseException:
                # Post-merge fix (MEDIUM-5): never let the loop thread die
                # silently — log loudly so the failure is observable.
                logging.exception(
                    "phase8-live-loop thread crashed in run_forever()"
                )
                raise

        self._live_loop = loop
        self._live_thread = threading.Thread(
            target=run_loop, name="phase8-live-loop", daemon=True
        )
        self._live_thread.start()
        return loop

    async def _start_live_components_async(self, adapter):
        if self._live_runner is not None and self._live_bridge is not None:
            return

        runner = LiveRunner(adapter=adapter, interval_ns=60 * 1_000_000_000)
        # D10: wire the event loop reference so fetch_instruments_blocking works
        runner._loop = self._live_loop
        bridge = LiveReducerBridge(bus=runner.bus, data_engine=self.engine)
        cache = LastPriceCache(bus=runner.bus)
        depth_cache = DepthCache(bus=runner.bus)
        await bridge.start()
        await cache.start()
        await depth_cache.start()
        await runner.start()
        self._live_runner = runner
        self._live_bridge = bridge
        self._live_price_cache = cache
        self._live_depth_cache = depth_cache
        # Phase 9 Step 2: manual order facade wraps this session's adapter.
        self._order_facade = ManualOrderFacade(adapter)
        # Phase 9 Step 4: account sync pushes AccountEvent on the backend stream.
        # The callback runs on the live-loop thread; BackendEventBus is threadsafe
        # (Step 0), so publishing directly from here is safe. AccountSync is
        # transport-agnostic — proto conversion + ts_ms stamping happens here.
        account_sync = AccountSync(
            adapter,
            on_account_event=self._publish_account_snapshot,
            interval_s=30.0,
        )
        await account_sync.start()
        self._account_sync = account_sync

    def _start_live_components(self, environment_hint: Optional[str] = None):
        if self._live_runner is not None and self._live_bridge is not None:
            return
        if self._live_adapter_factory is None:
            return
        # PermissionError on Windows can leak ttwr_cred_*.json from a prior
        # VenueLogin; sweep here where no concurrent login holds the file.
        _sweep_stale_cred_files()
        adapter = self._live_adapter_factory(environment_hint)
        loop = self._ensure_live_loop()
        future = asyncio.run_coroutine_threadsafe(
            self._start_live_components_async(adapter), loop
        )
        future.result(timeout=self._live_timeout_s)

    async def _teardown_live_components_async(self):
        bridge = self._live_bridge
        cache = self._live_price_cache
        depth_cache = self._live_depth_cache
        runner = self._live_runner
        account_sync = self._account_sync
        # Stop the account push first so no AccountEvent is emitted mid-teardown.
        if account_sync is not None:
            await account_sync.stop()
        if bridge is not None:
            await bridge.stop()
        if cache is not None:
            await cache.stop()
        if depth_cache is not None:
            await depth_cache.stop()
        if runner is not None:
            await runner.aclose()

    def _teardown_live_components(self):
        if self._live_runner is None and self._live_bridge is None:
            return
        loop = self._live_loop
        try:
            if loop is not None and loop.is_running():
                future = asyncio.run_coroutine_threadsafe(
                    self._teardown_live_components_async(), loop
                )
                future.result(timeout=self._live_timeout_s)
        except Exception:
            logging.exception("SetExecutionMode: failed to stop live components")
        finally:
            self._live_runner = None
            self._live_bridge = None
            self._live_price_cache = None
            self._live_depth_cache = None
            self._order_facade = None
            self._account_sync = None
            # Arm clear-on-toggle: a prior lifecycle's last_error must not bleed
            # into the next Live session or stay visible after returning to Replay.
            self._suppress_live_last_error = True
        # v5.2 Claim 2: reset venue_sm to DISCONNECTED so next Live entry
        # requires VenueLogin again (ensures adapter.is_logged_in invariant).
        if self.venue_sm is not None and self.venue_sm.current != "DISCONNECTED":
            self.venue_sm.reset()

    def Shutdown(self, request, context):
        # C-6: token 一致確認 → start_shutdown() 戻り値で 4 段判定
        if request.token != self.token:
            return engine_pb2.ShutdownResponse(
                accepted=False, error_code="INVALID_TOKEN"
            )
        grace = int(request.grace_seconds)
        if not process_lifecycle.start_shutdown(grace_seconds=grace):
            return engine_pb2.ShutdownResponse(
                accepted=False, error_code="ALREADY_SHUTTING_DOWN"
            )
        return engine_pb2.ShutdownResponse(accepted=True, error_code="")

    def Check(self, request, context):
        # C-1: service フィルタ。"" (default) と "DataEngine" のみ受理
        if request.service not in ("", "DataEngine"):
            return engine_pb2.HealthCheckResponse(
                status=engine_pb2.HealthCheckResponse.SERVICE_UNKNOWN
            )
        if process_lifecycle.is_shutting_down():
            return engine_pb2.HealthCheckResponse(
                status=engine_pb2.HealthCheckResponse.NOT_SERVING
            )
        return engine_pb2.HealthCheckResponse(
            status=engine_pb2.HealthCheckResponse.SERVING
        )

    def _resolve_live_last_error(self) -> Optional[BaseException]:
        err = self._live_runner.last_error if self._live_runner is not None else None
        if err is None and self._live_bridge is not None:
            err = self._live_bridge.last_error
        return err

    def GetState(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        err = self._resolve_live_last_error()
        # Clear-on-mode-toggle: while the suppression flag is set, GetState
        # reports None until a *new* error bubbles up from runner/bridge.
        if self._suppress_live_last_error and err is None:
            live_last_error = None
        elif self._suppress_live_last_error and err is not None:
            # A fresh error appeared after the suppression was armed — that
            # means the new lifecycle hit a real failure. Stop suppressing.
            self._suppress_live_last_error = False
            live_last_error = f"{type(err).__name__}: {err}"
        else:
            live_last_error = f"{type(err).__name__}: {err}" if err is not None else None

        # D8: mode-aware last_prices dispatch
        mode = self.mode_manager.current_mode if self.mode_manager else "Replay"
        state = self.engine.get_current_state()
        merged_pi = state.per_instrument
        if mode in ("LiveManual", "LiveAuto"):
            raw = (
                self._live_price_cache.snapshot()
                if self._live_price_cache is not None
                else {}
            )
            # D20 二段ガード: filter by subscribed_ids to prevent stale prices
            runner = self._live_runner
            if runner is not None:
                try:
                    subscribed = runner.subscribed_ids()
                    last_prices = {k: v for k, v in raw.items() if k in subscribed}
                except Exception:
                    last_prices = raw  # subscribed_ids broken → fall back
            else:
                last_prices = raw
            depth_by_id = (
                self._live_depth_cache.snapshot()
                if self._live_depth_cache is not None
                else {}
            )
            base_pi = state.per_instrument
            merged_pi = {
                k: (v.model_copy(update={"depth": d}) if (d := depth_by_id.get(k)) else v)
                for k, v in base_pi.items()
            }
            # depth はあるが kline 未着の銘柄 (base_pi に居ない) を補完
            for k, d in depth_by_id.items():
                if k not in merged_pi:
                    merged_pi[k] = PerInstrumentState(depth=d)
        else:  # Replay
            last_prices = self.engine.get_replay_last_prices()

        state = state.model_copy(
            update={
                "live_last_error": live_last_error,
                "last_prices": last_prices,
                "configured_venue": self._live_venue_id,
                "per_instrument": merged_pi,
            }
        )
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
            bars_by_instrument = load_bars_for_scenario(catalog_path, scenario)
            total_bars = sum(len(v) for v in bars_by_instrument.values())
            per_instrument = {str(k): len(v) for k, v in bars_by_instrument.items()}
            logging.info(
                f"StartEngine: bars loaded total={total_bars} per_instrument={per_instrument}"
            )

            base_dir = self.engine.jquants_loader_base_dir
            if base_dir:
                missing = [str(k) for k, v in bars_by_instrument.items() if not v]
                if missing:
                    gran = normalize_granularity(scenario["granularity"])
                    for symbol in missing:
                        try:
                            ensure_jquants_catalog(
                                base_dir=base_dir,
                                catalog_path=catalog_path,
                                instrument_id=symbol,
                                start_date=scenario["start"],
                                end_date=scenario["end"],
                                granularity=gran,
                            )
                        except (ValueError, FileNotFoundError) as e:
                            logging.warning("ensure_jquants_catalog skipped %s: %s", symbol, e)
                    bars_by_instrument = load_bars_for_scenario(catalog_path, scenario)

            still_missing = [str(k) for k, v in bars_by_instrument.items() if not v]
            if still_missing:
                return engine_pb2.StartEngineResponse(
                    success=False,
                    request_id=request.request_id,
                    current_state=self._current_engine_state(),
                    error_code="NO_BARS_AFTER_FALLBACK",
                    error_message=f"No bars after catalog fallback: {still_missing}",
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

                # D16: Expose ALL bars (all instruments) to GetState for multi-instrument chart.
                # bars[0] was already primed by _prime_provider_locked; inject bars[1:].
                # Primary instrument uses instrument_id="" to update history/price/ohlc
                # (backward compat). Secondary instruments use their actual id (per_id_close only).
                from .nautilus_adapter import bar_to_kline_update
                primary_iid = self.engine._replay_primary_id  # "" for legacy single-provider
                first = True
                for iid, bars in bars_by_instrument.items():
                    if not bars:
                        continue
                    iid_str = str(iid)
                    # primary instrument: also emit with "" id to update history
                    is_primary_instrument = first or (iid_str == primary_iid)
                    first = False
                    for bar in bars[1:]:
                        if is_primary_instrument:
                            # Emit with empty id → updates history/price/ohlc
                            self.engine.apply_replay_event(
                                bar_to_kline_update(bar, instrument_id="")
                            )
                        # Emit with actual id → updates per_id_close (D9)
                        self.engine.apply_replay_event(
                            bar_to_kline_update(bar, instrument_id=iid_str)
                        )

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

        # D1: source dispatch — "local" (default) vs "live"
        source = (getattr(request, "source", None) or "local").lower()
        if source not in {"local", "live"}:
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message=f"unknown source: {source}",
            )

        if source == "live":
            return self._list_instruments_live(context)
        return self._list_instruments_local(context)

    def _list_instruments_live(self, context):
        """D1/D10: Fetch instruments from live adapter (must be logged in)."""
        runner = self._live_runner
        if runner is None or not runner.is_logged_in():
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message="LIVE_VENUE_NOT_LOGGED_IN",
            )
        try:
            raws = runner.fetch_instruments_blocking(timeout=self._live_timeout_s)
        except Exception as exc:
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message=f"fetch_instruments failed: {exc}",
            )
        # v4 fix: empty list == adapter not implemented, treat as failure
        if not raws:
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message="LIVE_UNIVERSE_UNSUPPORTED",
            )
        instruments = [
            engine_pb2.Instrument(
                id=f"{r.code}.{r.market}",
                name=r.name,
                market=r.market,
            )
            for r in raws
        ]
        return engine_pb2.ListInstrumentsResponse(
            success=True,
            instrument_ids=[i.id for i in instruments],
            instruments=instruments,
        )

    def _list_instruments_local(self, context):
        """D1: List instruments from local catalog (existing logic)."""
        catalog_path = self.engine.last_replay_catalog_path or self.engine._jquants_catalog_path
        if not catalog_path:
            return engine_pb2.ListInstrumentsResponse(
                success=False,
                error_message="No catalog_path available",
            )

        try:
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
            instruments = [
                engine_pb2.Instrument(id=i, name=i, market="") for i in ids
            ]
            return engine_pb2.ListInstrumentsResponse(
                success=True,
                instrument_ids=ids,
                instruments=instruments,
            )
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

    async def _handle_prompt_login(
        self, venue_id: str, env_hint: str
    ) -> tuple[bool, str, Optional[str]]:
        """Spawn login_dialog_runner subprocess and handle cross-platform IPC.

        Returns (success, error_code, token_or_none).
        Tachibana: token_or_none is always None (uses session_cache on disk).
        Kabu: token_or_none is the bearer token from cred-path file.
        """
        cred_path = ""
        if venue_id.upper() == "KABU":
            fd, cred_path = tempfile.mkstemp(prefix="ttwr_cred_", suffix=".json")
            os.close(fd)
            if os.name == "posix":
                os.chmod(cred_path, 0o600)
        args = [
            sys.executable, "-m", "engine.live.login_dialog_runner",
            "--venue", venue_id.lower(),
            "--env", env_hint,
        ]
        if cred_path:
            args.extend(["--cred-path", cred_path])
        stderr_drain = None
        try:
            proc = await asyncio.create_subprocess_exec(
                *args,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stderr_drain = asyncio.ensure_future(proc.stderr.read())

            async def _drain_stderr_text() -> str:
                try:
                    data = await asyncio.wait_for(stderr_drain, timeout=5.0)
                except (asyncio.TimeoutError, asyncio.CancelledError):
                    data = b""
                return data.decode("utf-8", errors="replace")

            try:
                line = await asyncio.wait_for(
                    proc.stdout.readline(),
                    timeout=_live_login_timeout_s(),
                )
            except asyncio.TimeoutError:
                proc.kill()
                await proc.wait()
                stderr_drain.cancel()
                return False, "LOGIN_TIMEOUT", None

            if not line:
                logging.error(
                    "login_dialog_runner exited without result: %s",
                    await _drain_stderr_text(),
                )
                await proc.wait()
                return False, "LOGIN_SUBPROCESS_CRASHED", None

            try:
                result = json.loads(line)
            except json.JSONDecodeError:
                proc.kill()
                await proc.wait()
                logging.error(
                    "login_dialog_runner emitted non-JSON stdout: %s",
                    await _drain_stderr_text(),
                )
                return False, "LOGIN_INVALID_RESPONSE", None

            if not result.get("success"):
                try:
                    await asyncio.wait_for(proc.wait(), timeout=5.0)
                except asyncio.TimeoutError:
                    proc.kill()
                    await proc.wait()
                return False, result.get("error_code") or "AUTH_FAILED", None

            try:
                await asyncio.wait_for(proc.wait(), timeout=10.0)
            except asyncio.TimeoutError:
                proc.kill()
                await proc.wait()
                return False, "LOGIN_TIMEOUT", None

            if proc.returncode != 0:
                logging.warning(
                    "login_dialog_runner exited rc=%d after success-line: %s",
                    proc.returncode,
                    await _drain_stderr_text(),
                )
                return False, result.get("error_code") or "LOGIN_NONZERO_EXIT", None

            token: Optional[str] = None
            if cred_path:
                try:
                    with open(cred_path, "rb") as f:
                        blob = f.read()
                except OSError as exc:
                    logging.warning("cred_path read failed: %s", exc)
                    return False, "LOGIN_INVALID_RESPONSE", None
                if not blob:
                    return False, "LOGIN_INVALID_RESPONSE", None
                try:
                    payload = json.loads(blob.decode("utf-8"))
                except (json.JSONDecodeError, UnicodeDecodeError):
                    return False, "LOGIN_INVALID_RESPONSE", None
                if not isinstance(payload, dict):
                    return False, "LOGIN_INVALID_RESPONSE", None
                tok = payload.get("token")
                if not isinstance(tok, str) or not tok:
                    return False, "LOGIN_INVALID_RESPONSE", None
                token = tok
            return True, "", token
        finally:
            if stderr_drain is not None and not stderr_drain.done():
                stderr_drain.cancel()
                try:
                    await stderr_drain
                except (asyncio.CancelledError, Exception):
                    pass
            if cred_path:
                try:
                    os.unlink(cred_path)
                except FileNotFoundError:
                    pass
                except PermissionError:
                    logging.warning(
                        "cred_path leak (Windows handle race): %s — "
                        "stale file will be swept on next _start_live_components",
                        cred_path,
                    )

    def VenueLogin(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        cred_source = request.credentials_source or "prompt"
        if cred_source not in self._KNOWN_CRED_SOURCES:
            context.abort(
                grpc.StatusCode.INVALID_ARGUMENT,
                "INVALID_CREDENTIALS_SOURCE",
            )

        # D21: normalize venue_id to uppercase (UI sends lowercase "tachibana"/"kabu"/"mock")
        venue_id = (request.venue_id or "").upper()
        venue_state = self.venue_sm.current if self.venue_sm is not None else "DISCONNECTED"

        if venue_id not in self._KNOWN_VENUES:
            return engine_pb2.VenueLoginResponse(
                success=False, error_code="UNKNOWN_VENUE",
                venue_state=venue_state, instruments_loaded=0,
            )

        # Preserve backward compat: KABU session_cache is unsupported
        if venue_id == "KABU" and cred_source == "session_cache":
            return engine_pb2.VenueLoginResponse(
                success=False, error_code="UNSUPPORTED_FOR_VENUE",
                venue_state=venue_state, instruments_loaded=0,
            )

        # D26: validate against configured factory venue (1 backend = 1 venue)
        if self._live_adapter_factory is None:
            return engine_pb2.VenueLoginResponse(
                success=False, error_code="LIVE_ADAPTER_NOT_CONFIGURED",
                venue_state=venue_state, instruments_loaded=0,
            )

        configured_venue = (self._live_venue_id or venue_id).upper()
        if configured_venue != venue_id:
            return engine_pb2.VenueLoginResponse(
                success=False, error_code="VENUE_MISMATCH",
                venue_state=venue_state, instruments_loaded=0,
            )

        # Idempotent: already CONNECTED/SUBSCRIBED → no-op success — UNLESS the
        # runner/bridge died with a last_error. LiveRunner._run() never transitions
        # venue_sm to ERROR, so a crashed WS task leaves the state machine
        # stale-CONNECTED while no data flows. Detect the dead session, tear it
        # down, and fall through to a fresh login attempt so re-login recovers.
        if self.venue_sm is not None and self.venue_sm.current in ("CONNECTED", "SUBSCRIBED"):
            live_err = self._resolve_live_last_error()
            if live_err is None:
                return engine_pb2.VenueLoginResponse(
                    success=True, error_code="",
                    venue_state=self.venue_sm.current, instruments_loaded=0,
                )
            logging.warning(
                "VenueLogin: venue_sm=%s but live runner/bridge has last_error=%r; "
                "tearing down dead session to re-establish",
                self.venue_sm.current, live_err,
            )
            if self._live_runner is not None or self._live_bridge is not None:
                self._teardown_live_components()

        # AUTHENTICATING 中の二重起動防止
        if self.venue_sm is not None and self.venue_sm.current == "AUTHENTICATING":
            return engine_pb2.VenueLoginResponse(
                success=False, error_code="ALREADY_AUTHENTICATING",
                venue_state="AUTHENTICATING", instruments_loaded=0,
            )

        env_hint = getattr(request, "environment_hint", None) or None

        def _fail(error_code: str) -> engine_pb2.VenueLoginResponse:
            if self._live_runner is not None or self._live_bridge is not None:
                self._teardown_live_components()
            # _teardown_live_components only resets venue_sm when a live runner
            # existed; cover the "failed before _start_live_components" path so
            # AUTHENTICATING never sticks and dead-locks the next VenueLogin.
            if self.venue_sm is not None and self.venue_sm.current == "AUTHENTICATING":
                try:
                    self.venue_sm.transition_to("ERROR")
                except Exception:
                    pass
                self.venue_sm.reset()
            return engine_pb2.VenueLoginResponse(
                success=False, error_code=error_code,
                venue_state=self.venue_sm.current if self.venue_sm else "DISCONNECTED",
                instruments_loaded=0,
            )

        def _attempt(effective_source: str):
            """Returns (handled: bool, error_code: str).

            handled=True, error_code="" → success path
            handled=True, error_code!="" → _fail(error_code)
            handled=False, error_code="NO_DISPLAY_AVAILABLE" → retry with "env" (debug only)
            """
            try:
                self._start_live_components(environment_hint=env_hint)
                runner = self._live_runner
                adapter = runner.adapter
                loop = self._ensure_live_loop()

                if effective_source == "prompt":
                    if self.venue_sm is not None and self.venue_sm.current == "DISCONNECTED":
                        self.venue_sm.transition_to("AUTHENTICATING")

                    if venue_id == "TACHIBANA":
                        effective_env = env_hint if env_hint in ("demo", "prod") else "demo"
                    else:
                        effective_env = env_hint if env_hint in ("verify", "prod") else "verify"

                    fut = asyncio.run_coroutine_threadsafe(
                        self._handle_prompt_login(venue_id, effective_env),
                        loop,
                    )
                    # TODO(将来): VenueLoginStream で逐次 push できるようにする
                    success, ec, token = fut.result(timeout=_live_login_timeout_s() + 10)

                    if not success:
                        if ec == "NO_DISPLAY_AVAILABLE" and IS_DEBUG_BUILD:
                            return False, ec  # caller retries with "env"
                        return True, ec

                    from engine.live.adapter import VenueCredentials
                    if venue_id == "TACHIBANA":
                        adapter_creds = VenueCredentials(
                            credentials_source="session_cache",
                            environment_hint=effective_env,
                        )
                    else:
                        adapter_creds = VenueCredentials(
                            credentials_source="prompt_result",
                            environment_hint=effective_env,
                            token=token,
                        )
                else:
                    from engine.live.adapter import VenueCredentials
                    adapter_creds = VenueCredentials(
                        credentials_source=effective_source,
                        environment_hint=env_hint,
                    )

                if not adapter.is_logged_in:
                    login_fut = asyncio.run_coroutine_threadsafe(
                        adapter.login(adapter_creds), loop,
                    )
                    login_fut.result(timeout=_live_login_timeout_s())

                if self.venue_sm is not None and self.venue_sm.current == "DISCONNECTED":
                    self.venue_sm.transition_to("AUTHENTICATING")
                if self.venue_sm is not None and self.venue_sm.current == "AUTHENTICATING":
                    self.venue_sm.transition_to("CONNECTED")
                # Arm clear-on-toggle: suppress stale errors from a prior session.
                self._suppress_live_last_error = True
                return True, ""
            except ValueError as exc:
                # adapter 層が定義する判別可能エラーは error_code として透過。
                # それ以外の ValueError は VENUE_LOGIN_FAILED に丸める。
                code = str(exc)
                if code in _ADAPTER_ERROR_CODES:
                    logging.warning("VenueLogin adapter error (source=%s): %s", effective_source, code)
                    return True, code
                logging.exception("VenueLogin attempt failed (source=%s): %s", effective_source, exc)
                return True, "VENUE_LOGIN_FAILED"
            except Exception as exc:
                logging.exception("VenueLogin attempt failed (source=%s): %s", effective_source, exc)
                return True, "VENUE_LOGIN_FAILED"

        handled, error_code = _attempt(cred_source)
        if not handled and cred_source == "prompt":
            if IS_DEBUG_BUILD:
                if self._live_runner is not None or self._live_bridge is not None:
                    self._teardown_live_components()
                handled, error_code = _attempt("env")
            else:
                error_code = "NO_DISPLAY_AVAILABLE"

        if error_code:
            return _fail(error_code)

        return engine_pb2.VenueLoginResponse(
            success=True, error_code="",
            venue_state=self.venue_sm.current if self.venue_sm else "CONNECTED",
            instruments_loaded=0,
        )

    def VenueLogout(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        # Fix 1: stop live runner, bridge, price cache, and reset venue state machine
        if self._live_runner is not None or self._live_bridge is not None:
            self._teardown_live_components()
        elif self.venue_sm is not None and self.venue_sm.current != "DISCONNECTED":
            self.venue_sm.reset()
        return engine_pb2.VenueControlResponse(success=True, error_code="")

    def SetExecutionMode(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        if request.mode not in self._KNOWN_MODES:
            context.abort(grpc.StatusCode.INVALID_ARGUMENT, "INVALID_MODE")

        if self.mode_manager is None:
            return engine_pb2.SetExecutionModeResponse(
                success=False,
                error_code="NOT_IMPLEMENTED",
                execution_mode="",
            )

        if request.mode in ("LiveManual", "LiveAuto") and self._live_adapter_factory is None:
            return engine_pb2.SetExecutionModeResponse(
                success=False,
                error_code="LIVE_ADAPTER_NOT_CONFIGURED",
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
        if applied in ("LiveManual", "LiveAuto"):
            # D21: VenueLogin must have been called first. If runner is None, reject.
            if self._live_runner is None:
                return engine_pb2.SetExecutionModeResponse(
                    success=False,
                    error_code="VENUE_LOGIN_REQUIRED",
                    execution_mode="",
                )
        elif applied == "Replay" and (self._live_runner is not None or self._live_bridge is not None):
            self._teardown_live_components()

        return engine_pb2.SetExecutionModeResponse(
            success=True,
            error_code="",
            execution_mode=applied,
        )

    # kabuステーション API 上限 (R6). LiveRunner 自体に gating が無いので
    # servicer 層で拒否する。re-subscribe は cap 計算から外す。
    _MAX_LIVE_SUBSCRIPTIONS = 50

    def SubscribeMarketData(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        # Live runner 未起動 (Replay モード等) は precondition reject
        if self._live_runner is None:
            return engine_pb2.SubscribeResponse(
                success=False,
                error_code="EXECUTION_MODE_PRECONDITION",
            )
        # 50 銘柄 cap: 新規 instrument のみカウント (re-subscribe は no-op)
        try:
            already = self._live_runner.subscribed_ids()
        except Exception:
            already = set()
        if (
            request.instrument_id not in already
            and len(already) >= self._MAX_LIVE_SUBSCRIPTIONS
        ):
            return engine_pb2.SubscribeResponse(
                success=False,
                error_code="SUBSCRIPTION_LIMIT_EXCEEDED",
            )
        # request.channels は accept-and-ignore (LiveRunner 側で {"trades","depth"} 固定)
        loop = self._ensure_live_loop()
        try:
            future = asyncio.run_coroutine_threadsafe(
                self._live_runner.subscribe(request.instrument_id), loop
            )
            future.result(timeout=self._live_timeout_s)
        except Exception as exc:
            logging.exception("SubscribeMarketData failed: %s", exc)
            return engine_pb2.SubscribeResponse(
                success=False,
                error_code="SUBSCRIBE_FAILED",
            )
        return engine_pb2.SubscribeResponse(success=True, error_code="")

    def UnsubscribeMarketData(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        # Live runner 未起動 (Replay モード等) は precondition reject
        if self._live_runner is None:
            return engine_pb2.SubscribeResponse(
                success=False,
                error_code="EXECUTION_MODE_PRECONDITION",
            )
        loop = self._ensure_live_loop()
        try:
            future = asyncio.run_coroutine_threadsafe(
                self._live_runner.unsubscribe(request.instrument_id), loop
            )
            future.result(timeout=self._live_timeout_s)
        except Exception as exc:
            logging.exception("UnsubscribeMarketData failed: %s", exc)
            return engine_pb2.SubscribeResponse(
                success=False,
                error_code="UNSUBSCRIBE_FAILED",
            )
        # D20: remove from price + depth caches to prevent stale data on re-add
        if self._live_price_cache is not None:
            self._live_price_cache.remove(request.instrument_id)
        if self._live_depth_cache is not None:
            self._live_depth_cache.remove(request.instrument_id)
        # A0: drop reducer per-id state so the symbol stops surfacing in per_instrument
        self.engine.forget_instrument(request.instrument_id)
        return engine_pb2.SubscribeResponse(success=True, error_code="")

    def SubmitSecret(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        # secret は Res にもログにも残さない。
        try:
            self._secret_vault.submit(request.request_id, request.secret)
        except KeyError:
            logging.warning(
                "SubmitSecret: unknown request_id=%s", request.request_id
            )
            return engine_pb2.SubmitSecretRes(
                success=False, error_code="UNKNOWN_REQUEST_ID"
            )
        return engine_pb2.SubmitSecretRes(success=True, error_code="")

    # === Phase 9 Step 2: manual order execution facade ===

    @staticmethod
    def _order_event_to_proto(ev) -> "engine_pb2.OrderEvent":
        """Convert a transport-agnostic OrderEventData → proto OrderEvent."""
        return engine_pb2.OrderEvent(
            order_id=ev.order_id,
            venue_order_id=ev.venue_order_id,
            client_order_id=ev.client_order_id,
            status=ev.status,
            filled_qty=ev.filled_qty,
            avg_price=ev.avg_price,
            ts_ms=ev.ts_ms,
        )

    def _is_live_ordering_mode(self) -> bool:
        """Write order RPCs are allowed only in Live modes (Replay is rejected)."""
        mode = self.mode_manager.current_mode if self.mode_manager else "Replay"
        return mode in ("LiveManual", "LiveAuto")

    def _publish_account_snapshot(self, snapshot) -> None:
        """AccountSync callback: AccountSnapshot → proto AccountEvent → backend stream.

        Runs on the live-loop thread. The transport-agnostic snapshot has no ts_ms;
        stamp it here (push time). BackendEventBus is threadsafe (Step 0)."""
        proto = engine_pb2.AccountEvent(
            cash=snapshot.cash,
            buying_power=snapshot.buying_power,
            positions=[
                engine_pb2.AccountPosition(
                    symbol=p.symbol,
                    qty=p.qty,
                    avg_price=p.avg_price,
                    unrealized_pnl=p.unrealized_pnl,
                )
                for p in snapshot.positions
            ],
            ts_ms=int(time.time() * 1000),
        )
        self.publish_backend_event(engine_pb2.BackendEvent(account_event=proto))

    def PlaceOrder(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        # Replay (or no mode_manager) is structurally rejected — never reaches venue.
        if not self._is_live_ordering_mode():
            return engine_pb2.PlaceOrderRes(
                success=False, error_code="EXECUTION_MODE_PRECONDITION"
            )
        # Snapshot once: a concurrent SetExecutionMode→Replay teardown nulls
        # self._order_facade, so re-reading the attribute below would race
        # (TOCTOU → AttributeError). Bind the live reference here.
        facade = self._order_facade
        if facade is None:
            return engine_pb2.PlaceOrderRes(
                success=False, error_code="VENUE_LOGIN_REQUIRED"
            )

        # second_secret は facade に渡すが Step 2 では無視される（Step 5 で結線）。
        # ここでログに出さない（平文 secret の漏洩面を最小化）。
        second_secret = request.second_secret if request.HasField("second_secret") else None
        price = request.price if request.HasField("price") else None

        loop = self._ensure_live_loop()
        try:
            future = asyncio.run_coroutine_threadsafe(
                facade.place(
                    venue=request.venue,
                    instrument_id=request.instrument_id,
                    side=request.side,
                    qty=request.qty,
                    order_type=request.order_type,
                    time_in_force=request.time_in_force,
                    price=price,
                    second_secret=second_secret,
                ),
                loop,
            )
            event = future.result(timeout=self._live_timeout_s)
        except OrderFacadeError as exc:
            return engine_pb2.PlaceOrderRes(success=False, error_code=exc.error_code)
        except futures.TimeoutError:
            # 注文は venue 側で成立している可能性がある（reconcile は Step 8）。
            logging.warning("PlaceOrder timed out after %ss", self._live_timeout_s)
            return engine_pb2.PlaceOrderRes(success=False, error_code="PLACE_TIMEOUT")
        except Exception as exc:
            logging.exception("PlaceOrder failed: %s", exc)
            return engine_pb2.PlaceOrderRes(success=False, error_code="PLACE_FAILED")

        proto_ev = self._order_event_to_proto(event)
        # Push on the backend-event stream AND echo inline in the unary response.
        self.publish_backend_event(engine_pb2.BackendEvent(order_event=proto_ev))
        return engine_pb2.PlaceOrderRes(success=True, error_code="", order_event=proto_ev)

    def CancelOrder(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        if not self._is_live_ordering_mode():
            return engine_pb2.CancelOrderRes(
                success=False, error_code="EXECUTION_MODE_PRECONDITION"
            )
        # Snapshot once (see PlaceOrder): guard against concurrent teardown race.
        facade = self._order_facade
        if facade is None:
            return engine_pb2.CancelOrderRes(
                success=False, error_code="VENUE_LOGIN_REQUIRED"
            )

        second_secret = request.second_secret if request.HasField("second_secret") else None

        loop = self._ensure_live_loop()
        try:
            future = asyncio.run_coroutine_threadsafe(
                facade.cancel(
                    venue=request.venue,
                    order_id=request.order_id,
                    second_secret=second_secret,
                ),
                loop,
            )
            event = future.result(timeout=self._live_timeout_s)
        except OrderFacadeError as exc:
            return engine_pb2.CancelOrderRes(success=False, error_code=exc.error_code)
        except futures.TimeoutError:
            logging.warning("CancelOrder timed out after %ss", self._live_timeout_s)
            return engine_pb2.CancelOrderRes(success=False, error_code="CANCEL_TIMEOUT")
        except Exception as exc:
            logging.exception("CancelOrder failed: %s", exc)
            return engine_pb2.CancelOrderRes(success=False, error_code="CANCEL_FAILED")

        proto_ev = self._order_event_to_proto(event)
        self.publish_backend_event(engine_pb2.BackendEvent(order_event=proto_ev))
        return engine_pb2.CancelOrderRes(success=True, error_code="", order_event=proto_ev)

    def ModifyOrder(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        if not self._is_live_ordering_mode():
            return engine_pb2.ModifyOrderRes(
                success=False, error_code="EXECUTION_MODE_PRECONDITION"
            )
        # Snapshot once (see PlaceOrder): guard against concurrent teardown race.
        facade = self._order_facade
        if facade is None:
            return engine_pb2.ModifyOrderRes(
                success=False, error_code="VENUE_LOGIN_REQUIRED"
            )

        # Optional fields: HasField resolves "unset" vs "explicit value". A modify
        # with neither price nor qty is rejected by the facade (NOTHING_TO_MODIFY).
        new_price = request.new_price if request.HasField("new_price") else None
        new_qty = request.new_qty if request.HasField("new_qty") else None
        # second_secret は facade に渡すが Step 4 では無視される（Step 5 で結線）。
        # ここでログに出さない（平文 secret の漏洩面を最小化）。
        second_secret = (
            request.second_secret if request.HasField("second_secret") else None
        )

        loop = self._ensure_live_loop()
        try:
            future = asyncio.run_coroutine_threadsafe(
                facade.modify(
                    venue=request.venue,
                    order_id=request.order_id,
                    new_price=new_price,
                    new_qty=new_qty,
                    second_secret=second_secret,
                ),
                loop,
            )
            event = future.result(timeout=self._live_timeout_s)
        except OrderFacadeError as exc:
            return engine_pb2.ModifyOrderRes(success=False, error_code=exc.error_code)
        except futures.TimeoutError:
            logging.warning("ModifyOrder timed out after %ss", self._live_timeout_s)
            return engine_pb2.ModifyOrderRes(success=False, error_code="MODIFY_TIMEOUT")
        except Exception as exc:
            logging.exception("ModifyOrder failed: %s", exc)
            return engine_pb2.ModifyOrderRes(success=False, error_code="MODIFY_FAILED")

        proto_ev = self._order_event_to_proto(event)
        self.publish_backend_event(engine_pb2.BackendEvent(order_event=proto_ev))
        return engine_pb2.ModifyOrderRes(success=True, error_code="", order_event=proto_ev)

    def GetOrderStatus(self, request, context):
        # 読み取り系: Replay でも reject しない（§3.2）。live session が無ければ空応答。
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        # Snapshot once (see PlaceOrder): a concurrent teardown nulling
        # self._order_facade between this check and get_status() would otherwise
        # raise an uncaught AttributeError (surfaces as gRPC INTERNAL, not a
        # clean NO_LIVE_SESSION — this read handler has no try/except).
        facade = self._order_facade
        if facade is None:
            return engine_pb2.GetOrderStatusRes(
                success=False, error_code="NO_LIVE_SESSION"
            )
        event = facade.get_status(request.order_id)
        if event is None:
            return engine_pb2.GetOrderStatusRes(
                success=False, error_code="UNKNOWN_ORDER_ID"
            )
        return engine_pb2.GetOrderStatusRes(
            success=True,
            error_code="",
            order_event=self._order_event_to_proto(event),
        )

    def publish_backend_event(self, event):
        """Fan a BackendEvent out to all open SubscribeBackendEvents streams."""
        self._backend_event_bus.publish(event)

    def SubscribeBackendEvents(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        sub = self._backend_event_bus.subscribe()
        # Wake the blocked queue.get() in _Subscription.__next__ when the RPC
        # terminates (client cancel / deadline / stream teardown). Without this,
        # the worker thread parks forever in queue.get() and the at-exit
        # ThreadPoolExecutor join hangs the process. (Phase 9 Step 0 fix.)
        context.add_callback(sub.close)
        try:
            for event in sub:
                if not context.is_active():
                    break
                yield event
        finally:
            sub.close()


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
    live_venue: Optional[str] = None,
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

    live_adapter_factory = (
        build_live_adapter_factory(live_venue) if live_venue is not None else None
    )

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    servicer = GrpcDataEngineServer(
        token,
        engine,
        mode_manager=mm,
        venue_sm=venue_sm,
        live_adapter_factory=live_adapter_factory,
        live_venue_id=live_venue,
    )

    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)

    server.add_insecure_port(f"127.0.0.1:{port}")
    logging.info(f"Starting gRPC server on port {port}")

    # Step 3: shutdown 経路を process_lifecycle に集約
    process_lifecycle.set_components(server=server, engine=engine, servicer=servicer)

    def _on_signal(*_):
        process_lifecycle.start_shutdown()

    signal.signal(signal.SIGINT, _on_signal)
    if hasattr(signal, "SIGBREAK"):  # Windows only
        signal.signal(signal.SIGBREAK, _on_signal)

    server.start()
    print(f"GRPC_LISTENING port={port}", flush=True)
    server.wait_for_termination()
    return
