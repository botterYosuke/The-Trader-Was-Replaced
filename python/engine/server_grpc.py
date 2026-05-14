import logging
import threading
import time
from concurrent import futures
from typing import Optional

import grpc

from .core import DataEngine
from .proto import engine_pb2, engine_pb2_grpc
from .replay import BaseReplayProvider
from .jquants_loader import JQuantsLoader


class GrpcDataEngineServer(
    engine_pb2_grpc.HealthServicer, engine_pb2_grpc.DataEngineServicer
):
    def __init__(self, token: str, engine: DataEngine):
        self.token = token
        self.engine = engine

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
            return engine_pb2.ReplayControlResponse(
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
            return engine_pb2.ReplayControlResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="STRATEGY_FILE_NOT_FOUND",
                error_message=str(exc),
            )
        except Exception as exc:
            logging.error(f"StartEngine: strategy load failed: {exc}")
            return engine_pb2.ReplayControlResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="STRATEGY_LOAD_ERROR",
                error_message=str(exc),
            )

        catalog_path = self.engine.last_replay_catalog_path
        if not catalog_path:
            logging.error("StartEngine: catalog_path not available (LoadReplayData not called or no catalog configured)")
            return engine_pb2.ReplayControlResponse(
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
            return engine_pb2.ReplayControlResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="CATALOG_BARS_LOAD_ERROR",
                error_message=str(exc),
            )

        try:
            from pathlib import Path as _Path
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
                )
                rb.finish()

                summary = compute_summary(rb.run_dir)
                write_summary_json(rb.run_dir, summary)

                logging.info(
                    "StartEngine: run complete run_id=%s run_dir=%s summary=%r",
                    run_id,
                    rb.run_dir,
                    summary,
                )
            except Exception as exc:
                rb.abort()
                logging.exception("StartEngine: engine_runner failed")
                return engine_pb2.ReplayControlResponse(
                    success=False,
                    request_id=request.request_id,
                    current_state=self._current_engine_state(),
                    error_code="RUN_FAILED",
                    error_message=str(exc),
                )
        except ImportError as exc:
            logging.error("StartEngine: RunBuffer/engine_runner import failed: %s", exc)
            return engine_pb2.ReplayControlResponse(
                success=False,
                request_id=request.request_id,
                current_state=self._current_engine_state(),
                error_code="RUN_FAILED",
                error_message=str(exc),
            )

        success, error = self.engine.start_engine()
        return engine_pb2.ReplayControlResponse(
            success=success,
            request_id=request.request_id,
            current_state=self._current_engine_state(),
            error_code="" if success else "INVALID_STATE",
            error_message="" if success else error,
        )

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

    engine = DataEngine(
        replay_provider=replay_provider,
        max_history_len=max_history_len,
        jquants_loader=jquants_loader,
        jquants_catalog_path=jquants_catalog_path,
    )

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
    servicer = GrpcDataEngineServer(token, engine)

    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)

    server.add_insecure_port(f"[::]:{port}")
    logging.info(f"Starting gRPC server on port {port}")
    server.start()
    try:
        while True:
            time.sleep(86400)
    except KeyboardInterrupt:
        engine.stop()
        server.stop(0)
