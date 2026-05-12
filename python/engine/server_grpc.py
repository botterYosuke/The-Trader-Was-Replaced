import grpc
from concurrent import futures
import time
import logging
import json
from .proto import engine_pb2, engine_pb2_grpc
from .core import DataEngine

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
        return engine_pb2.GetStateResponse(json_data=json.dumps(state))

def serve(port: int, token: str):
    engine = DataEngine()
    engine.start()

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

