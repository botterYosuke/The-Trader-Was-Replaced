import json
import os
from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer
from engine.jquants_loader import JQuantsLoader

DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")


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


@pytest.fixture
def jquants_grpc_server():
    token = "test-token"
    loader = JQuantsLoader(DATA_DIR)
    engine = DataEngine(jquants_loader=loader)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine)

    server.stop(0)


def test_grpc_health_check(static_grpc_server):
    port, _, _ = static_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    health_stub = engine_pb2_grpc.HealthStub(channel)

    response = health_stub.Check(engine_pb2.HealthCheckRequest())
    assert response.status == engine_pb2.HealthCheckResponse.SERVING


def test_grpc_unauthenticated(static_grpc_server):
    port, _, _ = static_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    with pytest.raises(grpc.RpcError) as e:
        stub.Start(engine_pb2.StartRequest(token="wrong-token"))
    assert e.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_grpc_replay_control_flow(jquants_grpc_server):
    """
    Verifies the replay control happy path:
    LoadReplayData -> StartEngine -> PauseReplay -> StepReplay
    -> ResumeReplay -> StopReplay.
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-31",
            granularity=engine_pb2.DAILY,
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
    before_price = json.loads(before.json_data)["price"]

    step_resp = stub.StepReplay(
        engine_pb2.StepReplayRequest(
            request_id="step-1",
            token=token,
        )
    )
    assert step_resp.success
    assert step_resp.current_state == engine_pb2.PAUSED

    after_price = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)["price"]
    assert after_price != before_price

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


def test_grpc_step_replay_rejects_when_not_paused(static_grpc_server):
    """StepReplay should fail unless the replay state is PAUSED."""
    port, token, _ = static_grpc_server
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


def test_grpc_force_stop_replay_returns_to_idle(jquants_grpc_server):
    """ForceStopReplay should return the replay state to IDLE."""
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-force-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-31",
            granularity=engine_pb2.DAILY,
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


def test_grpc_start_engine_rejects_before_load(static_grpc_server):
    """StartEngine should fail before LoadReplayData moves the state to LOADED."""
    port, token, _ = static_grpc_server
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


def test_grpc_pause_replay_rejects_when_not_running(static_grpc_server):
    """PauseReplay should fail unless the replay state is RUNNING."""
    port, token, _ = static_grpc_server
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


def test_grpc_set_replay_speed_rejects_zero_multiplier(static_grpc_server):
    """SetReplaySpeed should reject a zero multiplier."""
    port, token, _ = static_grpc_server
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


def test_grpc_load_replay_data_succeeds_with_jquants_loader(jquants_grpc_server):
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-jquants-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-31",
            granularity=engine_pb2.DAILY,
        )
    )

    assert resp.success
    assert resp.current_state == engine_pb2.LOADED


def test_grpc_load_replay_data_succeeds_with_daily_granularity(jquants_grpc_server):
    """
    gRPC の DAILY granularity が equities_bars_daily_YYYYMM.csv.gz を正しく参照することを確認する。

    gRPC request の DAILY
      -> server_grpc.py で "Daily" に変換
      -> core.py に渡る
      -> JQuantsLoader が equities_bars_daily_202407.csv.gz を探す
      -> 見つかるので LOADED
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-jquants-daily-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-31",
            granularity=engine_pb2.DAILY,
        )
    )

    assert resp.success
    assert resp.current_state == engine_pb2.LOADED


@pytest.mark.slow
def test_grpc_load_replay_data_succeeds_with_minute_granularity(jquants_grpc_server):
    """
    gRPC の MINUTE granularity が equities_bars_minute_YYYYMM.csv.gz を正しく参照することを確認する。
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-jquants-minute-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-31",
            granularity=engine_pb2.MINUTE,
        )
    )

    assert resp.success
    assert resp.current_state == engine_pb2.LOADED


def test_grpc_load_replay_data_rejects_second_granularity(jquants_grpc_server):
    """SECOND granularity は未サポートとして拒否される。"""
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-second-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-31",
            granularity=engine_pb2.SECOND,
        )
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"
    assert "not supported" in resp.error_message


def test_grpc_daily_replay_advances_with_real_prices(jquants_grpc_server):
    """
    LoadReplayData(DAILY) → StartEngine → PauseReplay → StepReplay で
    実データの終値 3284.0 → 3333.0 と進むことを確認する。
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-daily-prices-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-02",
            granularity=engine_pb2.DAILY,
        )
    )
    assert load_resp.success
    assert load_resp.current_state == engine_pb2.LOADED

    stub.StartEngine(engine_pb2.StartEngineRequest(request_id="start-dp-1", token=token))
    stub.PauseReplay(engine_pb2.PauseReplayRequest(request_id="pause-dp-1", token=token))

    primed = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert primed["price"] == 3284.0

    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="step-dp-1", token=token))
    stepped = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert stepped["price"] == 3333.0


def test_grpc_daily_step_timestamp_and_history_points_sync(jquants_grpc_server):
    """
    Daily step 後に timestamp_ms と history_points が一致することを確認する。

    LoadReplayData(DAILY) 直後は _prime_provider_locked() が最初の tick を消費済み。
    そのため StartEngine → PauseReplay 直後の primed は 1 本目 (2024-07-01 15:30 JST)、
    StepReplay 後の stepped は 2 本目 (2024-07-02 15:30 JST) になる。
    """
    _JST_1530_20240701_MS = 1719815400000  # 2024-07-01 15:30:00 JST
    _JST_1530_20240702_MS = 1719901800000  # 2024-07-02 15:30:00 JST

    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-ts-sync-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-02",
            granularity=engine_pb2.DAILY,
        )
    )
    stub.StartEngine(engine_pb2.StartEngineRequest(request_id="start-ts-sync-1", token=token))
    stub.PauseReplay(engine_pb2.PauseReplayRequest(request_id="pause-ts-sync-1", token=token))

    primed = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert primed["price"] == 3284.0
    assert primed["timestamp_ms"] == _JST_1530_20240701_MS
    assert primed["history_points"][-1]["timestamp_ms"] == _JST_1530_20240701_MS
    assert primed["history_points"][-1]["price"] == 3284.0
    assert len(primed["history_points"]) == 1
    assert primed["history"] == [3284.0]

    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="step-ts-sync-1", token=token))
    stepped = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert stepped["price"] == 3333.0
    assert stepped["timestamp_ms"] == _JST_1530_20240702_MS
    assert stepped["history_points"][-1]["timestamp_ms"] == _JST_1530_20240702_MS
    assert stepped["history_points"][-1]["price"] == 3333.0
    assert len(stepped["history_points"]) == 2
    assert stepped["history"] == [3284.0, 3333.0]


@pytest.mark.slow
def test_grpc_minute_step_timestamp_and_history_points_sync(jquants_grpc_server):
    """
    Minute step 後に timestamp_ms と history_points が構造的に一致することを確認する。

    exact timestamp は Minute CSV の Time 列依存のため、ここでは関係的整合のみ検証する。
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-minute-ts-sync-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity=engine_pb2.MINUTE,
        )
    )
    stub.StartEngine(engine_pb2.StartEngineRequest(request_id="start-minute-ts-sync-1", token=token))
    stub.PauseReplay(engine_pb2.PauseReplayRequest(request_id="pause-minute-ts-sync-1", token=token))

    primed = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert primed["timestamp_ms"] > 0
    assert primed["history_points"][-1]["timestamp_ms"] == primed["timestamp_ms"]
    assert primed["history_points"][-1]["price"] == primed["price"]
    assert len(primed["history_points"]) == 1
    assert primed["history"] == [primed["price"]]

    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="step-minute-ts-sync-1", token=token))
    stepped = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert stepped["timestamp_ms"] > primed["timestamp_ms"]
    assert stepped["history_points"][-1]["timestamp_ms"] == stepped["timestamp_ms"]
    assert stepped["history_points"][-1]["price"] == stepped["price"]
    assert len(stepped["history_points"]) == 2
    assert stepped["history"] == [primed["price"], stepped["price"]]


@pytest.mark.slow
def test_grpc_minute_replay_advances_with_real_prices(jquants_grpc_server):
    """
    LoadReplayData(MINUTE) → StartEngine → PauseReplay → StepReplay で
    実データ 3308.0 → 3301.0 と進むことを確認する。
    """
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-minute-prices-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity=engine_pb2.MINUTE,
        )
    )
    assert load_resp.success
    assert load_resp.current_state == engine_pb2.LOADED

    stub.StartEngine(engine_pb2.StartEngineRequest(request_id="start-mp-1", token=token))
    stub.PauseReplay(engine_pb2.PauseReplayRequest(request_id="pause-mp-1", token=token))

    primed = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert primed["price"] == 3308.0

    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="step-mp-1", token=token))
    stepped = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert stepped["price"] == 3301.0


def test_grpc_stop_engine_aliases_stop_replay(jquants_grpc_server):
    port, token, _ = jquants_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    assert stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="load-stop-engine-1",
            token=token,
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-02",
            granularity=engine_pb2.DAILY,
        )
    ).success

    assert stub.StartEngine(
        engine_pb2.StartEngineRequest(request_id="start-stop-engine-1", token=token)
    ).success

    resp = stub.StopEngine(
        engine_pb2.StopEngineRequest(request_id="stop-engine-1", token=token)
    )

    assert resp.success
    assert resp.current_state == engine_pb2.IDLE


def test_grpc_stop_engine_rejects_from_idle(static_grpc_server):
    port, token, _ = static_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.StopEngine(
        engine_pb2.StopEngineRequest(request_id="stop-engine-idle-1", token=token)
    )

    assert not resp.success
    assert resp.current_state == engine_pb2.IDLE
    assert resp.error_code == "INVALID_STATE"


def test_grpc_load_replay_data_rejects_when_jquants_data_missing(tmp_path):
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
                granularity=engine_pb2.DAILY,
            )
        )

        assert not resp.success
        assert resp.current_state == engine_pb2.IDLE
        assert resp.error_code == "INVALID_STATE"
        assert "replay data" in resp.error_message.lower()
    finally:
        server.stop(0)
