import grpc
from concurrent import futures
import time
import logging
from .proto import engine_pb2, engine_pb2_grpc

class GrpcDataEngineServer(engine_pb2_grpc.HealthServicer, engine_pb2_grpc.DataEngineServicer):
    def __init__(self, token: str):
        self.token = token

    def Check(self, request, context):
        return engine_pb2.HealthCheckResponse(status=engine_pb2.HealthCheckResponse.SERVING)

    def GetState(self, request, context):
        if request.token != self.token:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
        
        # Phase 1: Return a simple fixed state
        sample_state = {
            "status": "active",
            "price": 100.0,
            "timestamp": int(time.time())
        }
        import json
        return engine_pb2.GetStateResponse(json_data=json.dumps(sample_state))

def serve(port: int, token: str):
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    servicer = GrpcDataEngineServer(token)
    
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    
    server.add_insecure_port(f'[::]:{port}')
    logging.info(f"Starting gRPC server on port {port}")
    server.start()
    try:
        while True:
            time.sleep(86400)
    except KeyboardInterrupt:
        server.stop(0)
