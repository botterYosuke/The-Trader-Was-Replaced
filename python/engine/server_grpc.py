import grpc
from concurrent import futures
import time
import logging
import threading
from typing import Optional
from .proto import engine_pb2, engine_pb2_grpc
from .core import DataEngine
from .replay import BaseReplayProvider

class GrpcDataEngineServer(engine_pb2_grpc.HealthServicer, engine_pb2_grpc.DataEngineServicer):
    def __init__(self, token: str, engine: DataEngine):
        self.token = token
        self.engine = engine

    def Check(self, request, context):
        return engine_pb2.HealthCheckResponse(status=engine_pb2.HealthCheckResponse.SERVING)

    def GetState(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

        state = self.engine.get_current_state()
        return engine_pb2.GetStateResponse(json_data=state.model_dump_json())

def advance_loop(engine: DataEngine, interval: float = 1.0):
    """エンジンを定期的に進行させるバックグラウンドループ"""
    logging.info(f"Starting advance loop with interval {interval}s")
    while engine.is_running:
        engine.advance()
        time.sleep(interval)
    logging.info("Advance loop stopped")

def serve(port: int, token: str, replay_provider: Optional[BaseReplayProvider] = None):
    engine = DataEngine(replay_provider=replay_provider)
    engine.start()

    # Replay モードの場合のみ自動進行させる。
    # Static モードは Phase 1/2 の固定価格契約を維持するため自動進行しない。
    if engine.mode == "replay":
        ticker_thread = threading.Thread(
            target=advance_loop, 
            args=(engine, 1.0), 
            daemon=True
        )
        ticker_thread.start()
    else:
        logging.info("Static mode: Automatic advance disabled")

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    servicer = GrpcDataEngineServer(token, engine)

    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)

    server.add_insecure_port(f'[::]:{port}')
    logging.info(f"Starting gRPC server on port {port}")
    server.start()
    try:
        while True:
            time.sleep(86400)
    except KeyboardInterrupt:
        engine.stop()
        server.stop(0)
