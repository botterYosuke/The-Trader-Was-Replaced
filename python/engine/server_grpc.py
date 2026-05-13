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
        if granularity == engine_pb2.MINUTE:
            return "Minute"
        if granularity == engine_pb2.DAILY:
            return "Daily"
        return "Trade"

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

        success, error = self.engine.load_replay_data(
            request.instrument_ids,
            request.start_date,
            request.end_date,
            self._replay_granularity_name(request.granularity),
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

        success, error = self.engine.start_engine()
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
):
    jquants_loader = JQuantsLoader(jquants_dir) if jquants_dir else None

    engine = DataEngine(
        replay_provider=replay_provider,
        max_history_len=max_history_len,
        jquants_loader=jquants_loader,
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
