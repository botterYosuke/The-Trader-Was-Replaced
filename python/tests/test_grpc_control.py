import pytest
import grpc
import time
import threading
from concurrent import futures
from engine.core import DataEngine
from engine.server_grpc import GrpcDataEngineServer, advance_loop
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.replay import SimpleCSVProvider

@pytest.fixture
def csv_file(tmp_path):
    f = tmp_path / "test.csv"
    f.write_text("timestamp,price\n1600000000,100.0\n1600000001,101.0\n1600000002,102.0\n")
    return str(f)

@pytest.fixture
def grpc_server(csv_file):
    token = "test-token"
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    
    port = 50051
    server.add_insecure_port(f'[::]:{port}')
    server.start()
    
    ticker_thread = threading.Thread(
        target=advance_loop, 
        args=(engine, 0.1), # Fast interval for testing
        daemon=True
    )
    ticker_thread.start()
    
    yield (port, token, engine)
    
    server.stop(0)

def test_grpc_control_flow(grpc_server):
    port, token, engine = grpc_server
    channel = grpc.insecure_channel(f'localhost:{port}')
    stub = engine_pb2_grpc.DataEngineStub(channel)
    
    # Initially not running
    assert not engine.is_running
    
    # Get state - should be primed (price 100)
    response = stub.GetState(engine_pb2.GetStateRequest(token=token))
    import json
    data = json.loads(response.json_data)
    assert data["price"] == 100.0
    
    # Start engine via gRPC
    start_resp = stub.Start(engine_pb2.StartRequest(token=token))
    assert start_resp.success
    assert engine.is_running
    
    # Wait for progression
    time.sleep(0.3)
    response = stub.GetState(engine_pb2.GetStateRequest(token=token))
    data = json.loads(response.json_data)
    assert data["price"] > 100.0 # Should have advanced
    
    # Stop engine via gRPC
    stop_resp = stub.Stop(engine_pb2.StopRequest(token=token))
    assert stop_resp.success
    assert not engine.is_running
    
    last_price = data["price"]
    time.sleep(0.3)
    response = stub.GetState(engine_pb2.GetStateRequest(token=token))
    data = json.loads(response.json_data)
    assert data["price"] == last_price # Should have stopped

def test_grpc_unauthenticated(grpc_server):
    port, _, _ = grpc_server
    channel = grpc.insecure_channel(f'localhost:{port}')
    stub = engine_pb2_grpc.DataEngineStub(channel)
    
    with pytest.raises(grpc.RpcError) as e:
        stub.Start(engine_pb2.StartRequest(token="wrong-token"))
    assert e.value.code() == grpc.StatusCode.UNAUTHENTICATED
