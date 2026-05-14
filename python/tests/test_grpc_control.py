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

_STRATEGY_7203_DAILY = os.path.join(DATA_DIR, "test_strategy_7203_daily.py")
_STRATEGY_7203_MINUTE = os.path.join(DATA_DIR, "test_strategy_7203_minute.py")


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
def jquants_grpc_server(tmp_path):
    token = "test-token"
    loader = JQuantsLoader(DATA_DIR)
    engine = DataEngine(
        jquants_loader=loader,
        jquants_catalog_path=str(tmp_path / "catalog"),
    )

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


@pytest.mark.slow
def test_grpc_replay_control_flow(jquants_grpc_server):
    """
    Verifies the replay control happy path under the current StartEngine contract:
    LoadReplayData -> LOADED, StartEngine(with strategy) -> runs strategy to
    completion -> returns IDLE with run_id populated.

    Note: StartEngine is synchronous and calls force_stop_replay() on completion,
    so PauseReplay / StepReplay after StartEngine are no longer applicable.
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
            config=engine_pb2.EngineStartConfig(
                strategy_file=_STRATEGY_7203_DAILY,
                instrument_ids=["7203.TSE"],
                granularity=engine_pb2.DAILY,
            ),
        )
    )
    assert start_resp.success
    # StartEngine runs strategy to completion and force_stop_replay() returns to IDLE.
    assert start_resp.current_state == engine_pb2.IDLE
    assert start_resp.run_id  # run_id is set after successful strategy execution

    # History is populated with bars from the strategy run.
    state = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert len(state["history"]) >= 1


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


@pytest.mark.slow
def test_grpc_force_stop_replay_returns_to_idle(jquants_grpc_server):
    """ForceStopReplay should return the replay state to IDLE from any state.

    Tests force-stop from LOADED (after LoadReplayData, before StartEngine).
    ForceStop is unconditional — it resets state regardless of current value.
    """
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
    assert load_resp.current_state == engine_pb2.LOADED

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
    """StartEngine should fail before LoadReplayData moves the state to LOADED.

    Under the current contract, config.strategy_file is validated first, so the
    error is MISSING_STRATEGY_FILE (state is still IDLE).
    """
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
    # strategy_file is checked before the LOADED state guard, so MISSING_STRATEGY_FILE fires first.
    assert resp.error_code == "MISSING_STRATEGY_FILE"
    assert "strategy_file" in resp.error_message


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


@pytest.mark.slow
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


@pytest.mark.slow
def test_grpc_load_replay_data_succeeds_with_daily_granularity(jquants_grpc_server):
    """
    gRPC の DAILY granularity が catalog 経由で LOADED になることを確認する。

    gRPC request の DAILY
      -> server_grpc.py で "Daily" に変換
      -> core.py が ensure_jquants_catalog -> NautilusBarsReplayProvider
      -> LOADED
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


@pytest.mark.slow
def test_grpc_daily_replay_advances_with_real_prices(jquants_grpc_server):
    """
    LoadReplayData(DAILY) + StartEngine(with strategy) で strategy が完走し、
    history に実データの終値 3284.0 → 3333.0 が順番に記録されることを確認する。

    StartEngine は synchronous であり完走後 IDLE に戻るため、
    LoadReplayData で primed された bar[0] と bars[1:] が history に入る。
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

    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-dp-1",
            token=token,
            config=engine_pb2.EngineStartConfig(
                strategy_file=_STRATEGY_7203_DAILY,
                instrument_ids=["7203.TSE"],
                granularity=engine_pb2.DAILY,
            ),
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.IDLE

    state = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert state["history"][0] == 3284.0   # first bar (primed by LoadReplayData)
    assert state["history"][-1] == 3333.0  # last bar (2024-07-02)


@pytest.mark.slow
def test_grpc_daily_step_timestamp_and_history_points_sync(jquants_grpc_server):
    """
    LoadReplayData(DAILY) + StartEngine(with strategy) の完走後、
    history_points に timestamp_ms と price が正しく記録されることを確認する。

    - bar[0] は LoadReplayData の _prime_provider_locked() で history に入る。
    - bars[1:] は StartEngine 完走後に apply_replay_event() で history に追加される。
    - StartEngine 完走後は state = IDLE。
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
    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-ts-sync-1",
            token=token,
            config=engine_pb2.EngineStartConfig(
                strategy_file=_STRATEGY_7203_DAILY,
                instrument_ids=["7203.TSE"],
                granularity=engine_pb2.DAILY,
            ),
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.IDLE

    state = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)

    # After StartEngine: all bars are in history.
    assert len(state["history_points"]) == 2
    assert state["history_points"][0]["timestamp_ms"] == _JST_1530_20240701_MS
    assert state["history_points"][0]["price"] == 3284.0
    assert state["history_points"][1]["timestamp_ms"] == _JST_1530_20240702_MS
    assert state["history_points"][1]["price"] == 3333.0
    assert state["history"] == [3284.0, 3333.0]
    assert state["timestamp_ms"] == _JST_1530_20240702_MS
    assert state["price"] == 3333.0


@pytest.mark.slow
def test_grpc_minute_step_timestamp_and_history_points_sync(jquants_grpc_server):
    """
    LoadReplayData(MINUTE) + StartEngine(with strategy) の完走後、
    history_points が構造的に一致することを確認する。

    exact timestamp は Minute CSV の Time 列依存のため、ここでは関係的整合のみ検証する。
    StartEngine 完走後は state = IDLE。
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
    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-minute-ts-sync-1",
            token=token,
            config=engine_pb2.EngineStartConfig(
                strategy_file=_STRATEGY_7203_MINUTE,
                instrument_ids=["7203.TSE"],
                granularity=engine_pb2.MINUTE,
            ),
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.IDLE

    state = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)

    # After StartEngine: all minute bars are in history (2+ bars expected).
    assert len(state["history_points"]) >= 2
    assert state["timestamp_ms"] > 0
    assert state["history_points"][-1]["timestamp_ms"] == state["timestamp_ms"]
    assert state["history_points"][-1]["price"] == state["price"]
    # Timestamps are strictly increasing.
    timestamps = [p["timestamp_ms"] for p in state["history_points"]]
    assert timestamps == sorted(timestamps)
    assert len(set(timestamps)) == len(timestamps)


@pytest.mark.slow
def test_grpc_minute_replay_advances_with_real_prices(jquants_grpc_server):
    """
    LoadReplayData(MINUTE) + StartEngine(with strategy) で strategy が完走し、
    history に実データの終値 3308.0, 3301.0 が記録されることを確認する。

    bar[0]=3308.0 は LoadReplayData で primed, bar[1]=3301.0 は StartEngine 後に history に入る。
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

    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-mp-1",
            token=token,
            config=engine_pb2.EngineStartConfig(
                strategy_file=_STRATEGY_7203_MINUTE,
                instrument_ids=["7203.TSE"],
                granularity=engine_pb2.MINUTE,
            ),
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.IDLE

    state = json.loads(stub.GetState(engine_pb2.GetStateRequest(token=token)).json_data)
    assert state["history"][0] == 3308.0   # first minute bar (primed by LoadReplayData)
    assert 3301.0 in state["history"]      # second minute bar


@pytest.mark.slow
def test_grpc_stop_engine_aliases_stop_replay(jquants_grpc_server):
    """StopEngine delegates to stop_replay(); enforces the same state guard.

    StartEngine is synchronous and calls force_stop_replay() on completion, so the
    state is already IDLE when StopEngine is called. StopEngine returns INVALID_STATE
    (same as StopReplay from IDLE), confirming the alias behaviour.
    """
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

    start_resp = stub.StartEngine(
        engine_pb2.StartEngineRequest(
            request_id="start-stop-engine-1",
            token=token,
            config=engine_pb2.EngineStartConfig(
                strategy_file=_STRATEGY_7203_DAILY,
                instrument_ids=["7203.TSE"],
                granularity=engine_pb2.DAILY,
            ),
        )
    )
    assert start_resp.success
    assert start_resp.current_state == engine_pb2.IDLE

    # StartEngine already returned to IDLE; StopEngine (→ stop_replay) rejects from IDLE.
    resp = stub.StopEngine(
        engine_pb2.StopEngineRequest(request_id="stop-engine-1", token=token)
    )

    assert not resp.success
    assert resp.error_code == "INVALID_STATE"
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
    engine = DataEngine(
        jquants_loader=loader,
        jquants_catalog_path=str(tmp_path / "catalog"),
    )

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
        # ensure_jquants_catalog raises "No daily rows for..." when CSV is missing
        assert "rows" in resp.error_message.lower()
    finally:
        server.stop(0)
