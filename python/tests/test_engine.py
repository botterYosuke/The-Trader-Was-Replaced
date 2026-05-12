import pytest
import subprocess
import time
import grpc
from engine.proto import engine_pb2, engine_pb2_grpc

@pytest.fixture(scope="module")
def grpc_server():
    port = 19877
    token = "test-token"
    # Start the server as a subprocess
    process = subprocess.Popen([
        "uv", "run", "python", "-m", "engine",
        "--port", str(port),
        "--token", token
    ], cwd=".")
    
    # Wait for the server to start
    time.sleep(2)
    
    yield (port, token)
    
    # Stop the server
    process.terminate()
    process.wait()

def test_health_check(grpc_server):
    port, _ = grpc_server
    with grpc.insecure_channel(f"localhost:{port}") as channel:
        stub = engine_pb2_grpc.HealthStub(channel)
        response = stub.Check(engine_pb2.HealthCheckRequest(service=""))
        assert response.status == engine_pb2.HealthCheckResponse.SERVING

def test_get_state_success(grpc_server):
    port, token = grpc_server
    with grpc.insecure_channel(f"localhost:{port}") as channel:
        stub = engine_pb2_grpc.DataEngineStub(channel)
        response = stub.GetState(engine_pb2.GetStateRequest(token=token))
        assert response.json_data is not None
        import json
        data = json.loads(response.json_data)
        assert data["status"] == "active"
        assert "price" in data

def test_get_state_unauthenticated(grpc_server):
    port, _ = grpc_server
    with grpc.insecure_channel(f"localhost:{port}") as channel:
        stub = engine_pb2_grpc.DataEngineStub(channel)
        with pytest.raises(grpc.RpcError) as e:
            stub.GetState(engine_pb2.GetStateRequest(token="wrong-token"))
        assert e.value.code() == grpc.StatusCode.UNAUTHENTICATED
