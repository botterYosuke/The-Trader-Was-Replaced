import pytest
import subprocess
import time
import grpc
import json
import sys
from pathlib import Path
from engine.proto import engine_pb2, engine_pb2_grpc

# Set the working directory to the 'python' directory relative to this test file
PYTHON_DIR = Path(__file__).parent.parent.resolve()

@pytest.fixture(scope="module")
def grpc_server():
    port = 19877
    token = "test-token"
    # Start the server as a subprocess using sys.executable
    process = subprocess.Popen([
        sys.executable, "-m", "engine",
        "--port", str(port),
        "--token", token
    ], cwd=str(PYTHON_DIR))
    
    # Wait for the server to start (polling health check)
    max_retries = 10
    retry_interval = 0.5
    for i in range(max_retries):
        try:
            with grpc.insecure_channel(f"localhost:{port}") as channel:
                stub = engine_pb2_grpc.HealthStub(channel)
                response = stub.Check(engine_pb2.HealthCheckRequest(service=""), timeout=1.0)
                if response.status == engine_pb2.HealthCheckResponse.SERVING:
                    break
        except Exception:
            pass
        time.sleep(retry_interval)
    else:
        process.terminate()
        process.wait()
        pytest.fail("gRPC server failed to start or health check timed out")
    
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
        data = json.loads(response.json_data)
        
        # Verify specific contract fields and types
        assert isinstance(data["price"], (int, float))
        assert data["price"] == 120.5
        assert isinstance(data["history"], list)
        assert all(isinstance(x, (int, float)) for x in data["history"])
        assert data["history"] == [118.0, 119.0, 121.0, 120.5]
        assert isinstance(data["timer"], (int, float))
        assert data["timer"] == 42.0

def test_get_state_unauthenticated(grpc_server):
    port, _ = grpc_server
    with grpc.insecure_channel(f"localhost:{port}") as channel:
        stub = engine_pb2_grpc.DataEngineStub(channel)
        with pytest.raises(grpc.RpcError) as e:
            stub.GetState(engine_pb2.GetStateRequest(token="wrong-token"))
        assert e.value.code() == grpc.StatusCode.UNAUTHENTICATED

def test_cli_missing_token():
    # Expect error exit when --token is missing
    result = subprocess.run([
        sys.executable, "-m", "engine", "--port", "19878"
    ], cwd=str(PYTHON_DIR), capture_output=True, text=True)
    assert result.returncode != 0
    # Assertion more flexible for different locales/argparse versions
    output = (result.stderr + result.stdout).lower()
    assert "--token" in output
    assert any(word in output for word in ["required", "必要な", "引数", "error"])

def test_cli_invalid_transport():
    # Expect error exit when --transport is invalid
    result = subprocess.run([
        sys.executable, "-m", "engine", "--token", "test", "--transport", "invalid"
    ], cwd=str(PYTHON_DIR), capture_output=True, text=True)
    assert result.returncode != 0
    output = (result.stderr + result.stdout).lower()
    assert any(word in output for word in ["invalid choice", "無効な", "選択肢", "error"])
