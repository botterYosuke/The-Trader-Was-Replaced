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

def advance_loop(engine: DataEngine, interval: float = 1.0):
    """
    エンジンを定期的に進行させるバックグラウンドループ。
    e-station の設計思想に基づき、1回待機してから進行させる。
    """
    logging.info(f"Starting advance loop with interval {interval}s")
    while True:
        # まず待機することで、初期状態 (Primed state) を取得する猶予を与える
        time.sleep(interval)
        # engine.is_running は thread-safe property になった
        if engine.is_running:
            engine.advance()
    logging.info("Advance loop stopped")

def serve(port: int, token: str, replay_provider: Optional[BaseReplayProvider] = None, auto_start: bool = False):
    engine = DataEngine(replay_provider=replay_provider)
    
    # サーバー起動時点ではエンジンを一時停止状態にしておく（auto_start が False の場合）
    if auto_start:
        engine.start()
    else:
        logging.info("Engine initialized in paused state.")

    ticker_thread = threading.Thread(
        target=advance_loop, 
        args=(engine, 1.0), 
        daemon=True
    )
    ticker_thread.start()

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
