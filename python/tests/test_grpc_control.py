import json
import threading
import time
from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.replay import SimpleCSVProvider
from engine.server_grpc import GrpcDataEngineServer, advance_loop
from engine.jquants_loader import JQuantsLoader


@pytest.fixture
def csv_file(tmp_path):
    f = tmp_path / "test.csv"
    f.write_text(
        "timestamp,price\n1600000000,100.0\n1600000001,101.0\n1600000002,102.0\n"
    )
    return str(f)


@pytest.fixture
def grpc_server(csv_file):
    token = "test-token"
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    ticker_thread = threading.Thread(
        target=advance_loop,
        args=(engine, 0.1),
        daemon=True,
    )
    ticker_thread.start()

    yield (port, token, engine)

    server.stop(0)


@pytest.fixture
def static_grpc_server():
    token = "test-token"
    engine = DataEngine()

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine)

    server.stop(0)


def test_grpc_health_check(grpc_server):
    port, _, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    health_stub = engine_pb2_grpc.HealthStub(channel)

    response = health_stub.Check(engine_pb2.HealthCheckRequest())
    assert response.status == engine_pb2.HealthCheckResponse.SERVING


def test_grpc_control_flow(grpc_server):
    port, token, engine = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    assert not engine.is_running

    response = stub.GetState(engine_pb2.GetStateRequest(token=token))
    data = json.loads(response.json_data)
    assert data["price"] == 100.0

    start_resp = stub.Start(engine_pb2.StartRequest(token=token))
    assert start_resp.success
    assert engine.is_running

    time.sleep(0.3)
    response = stub.GetState(engine_pb2.GetStateRequest(token=token))
    data = json.loads(response.json_data)
    assert data["price"] > 100.0

    stop_resp = stub.Stop(engine_pb2.StopRequest(token=token))
    assert stop_resp.success
    assert not engine.is_running

    last_price = data["price"]
    time.sleep(0.3)
    response = stub.GetState(engine_pb2.GetStateRequest(token=token))
    data = json.loads(response.json_data)
    assert data["price"] == last_price


def test_grpc_unauthenticated(grpc_server):
    port, _, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    with pytest.raises(grpc.RpcError) as e:
        stub.Start(engine_pb2.StartRequest(token="wrong-token"))
    assert e.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_grpc_replay_control_flow(grpc_server):
    """
    Verifies the replay control happy path:
    LoadReplayData -> StartEngine -> PauseReplay -> StepReplay
    -> ResumeReplay -> StopReplay.
    """
    port, token, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-1",
            token=token,
            instrument_ids=["TEST"],
            start_date="2024-01-01",
            end_date="2024-01-02",
        )
    )
    assert load_resp.success
    assert load_resp.current_state == engine_pb2.LOADED

    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-1",
            token=token,
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.RUNNING

    pause_resp = stub.PauseReplay(
        engine_pb2.PauseReplayRequest(
            request_id="pause-1",
            token=token,
        )
    )
    assert pause_resp.success
    assert pause_resp.current_state == engine_pb2.PAUSED

    before = stub.GetState(engine_pb2.GetStateRequest(token=token))
    before_data = json.loads(before.json_data)
    before_price = before_data["price"]

    step_resp = stub.StepReplay(
        engine_pb2.StepReplayRequest(
            request_id="step-1",
            token=token,
        )
    )
    assert step_resp.success
    assert step_resp.current_state == engine_pb2.PAUSED

    after = stub.GetState(engine_pb2.GetStateRequest(token=token))
    after_data = json.loads(after.json_data)
    assert after_data["price"] != before_price

    resume_resp = stub.ResumeReplay(
        engine_pb2.ResumeReplayRequest(
            request_id="resume-1",
            token=token,
        )
    )
    assert resume_resp.success
    assert resume_resp.current_state == engine_pb2.RUNNING

    stop_resp = stub.StopReplay(
        engine_pb2.StopReplayRequest(
            request_id="stop-1",
            token=token,
        )
    )
    assert stop_resp.success
    assert stop_resp.current_state == engine_pb2.IDLE


def test_grpc_step_replay_rejects_when_not_paused(grpc_server):
    """StepReplay should fail unless the replay state is PAUSED."""
    port, token, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.StepReplay(
        engine_pb2.StepReplayRequest(
            request_id="bad-step-1",
            token=token,
        )
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"
    assert "PAUSED" in resp.error_message


def test_grpc_force_stop_replay_returns_to_idle(grpc_server):
    """ForceStopReplay should return the replay state to IDLE."""
    port, token, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-force-1",
            token=token,
            instrument_ids=["TEST"],
            start_date="2024-01-01",
            end_date="2024-01-02",
        )
    )
    assert load_resp.success

    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-force-1",
            token=token,
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.RUNNING

    force_resp = stub.ForceStopReplay(
        engine_pb2.ForceStopReplayRequest(
            request_id="force-stop-1",
            token=token,
        )
    )
    assert force_resp.success
    assert force_resp.current_state == engine_pb2.IDLE


def test_grpc_load_replay_data_rejects_without_replay_provider(static_grpc_server):
    """LoadReplayData should fail when no replay provider is configured."""
    port, token, _ = static_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-without-provider-1",
            token=token,
            instrument_ids=["TEST"],
            start_date="2024-01-01",
            end_date="2024-01-02",
        )
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"
    assert "Replay provider" in resp.error_message


def test_grpc_start_engine_rejects_before_load(grpc_server):
    """StartEngine should fail before LoadReplayData moves the state to LOADED."""
    port, token, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-before-load-1",
            token=token,
        )
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"
    assert "LOADED" in resp.error_message


def test_grpc_pause_replay_rejects_when_not_running(grpc_server):
    """PauseReplay should fail unless the replay state is RUNNING."""
    port, token, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.PauseReplay(
        engine_pb2.PauseReplayRequest(
            request_id="pause-before-start-1",
            token=token,
        )
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"
    assert "RUNNING" in resp.error_message


def test_grpc_set_replay_speed_rejects_zero_multiplier(grpc_server):
    """SetReplaySpeed should reject a zero multiplier."""
    port, token, _ = grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.SetReplaySpeed(
        engine_pb2.SetReplaySpeedRequest(
            request_id="speed-zero-1",
            token=token,
            multiplier=0,
        )
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"
    assert "multiplier" in resp.error_message


@pytest.fixture
def jquants_grpc_server(tmp_path):
    token = "test-token"
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()

    loader = JQuantsLoader(str(base_dir))
    engine = DataEngine(jquants_loader=loader)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine)

    server.stop(0)


def test_grpc_load_replay_data_succeeds_with_jquants_loader(jquants_grpc_server):
    """
    成功テスト
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-jquants-1",
            token=token,
            instrument_ids=["7203"],
            start_date="2024-01-01",
            end_date="2024-01-31",
        )
    )

    assert resp.success
    assert resp.current_state == engine_pb2.LOADED


def test_grpc_load_replay_data_rejects_when_jquants_data_missing(tmp_path):
    """
    失敗テスト
    """
    token = "test-token"
    loader = JQuantsLoader(str(tmp_path / "missing-j-quants"))
    engine = DataEngine(jquants_loader=loader)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    try:
        channel = grpc.insecure_channel(f"localhost:{port}")
        stub = engine_pb2_grpc.DataEngineStub(channel)

        resp = stub.LoadReplayData(
            engine_pb2.LoadReplayDataRequest(
                request_id="load-jquants-missing-1",
                token=token,
                instrument_ids=["7203"],
                start_date="2024-01-01",
                end_date="2024-01-31",
            )
        )

        assert not resp.success
        assert resp.current_state == engine_pb2.IDLE
        assert resp.error_code == "INVALID_STATE"
        assert "Replay data" in resp.error_message
    finally:
        server.stop(0)


def test_check_data_exists_accepts_multiple_instrument_ids(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203", "6758"],
        start_date="2024-01-01",
        end_date="2024-01-31",
    )
