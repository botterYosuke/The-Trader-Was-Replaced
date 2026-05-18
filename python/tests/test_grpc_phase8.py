import asyncio
import json
from unittest.mock import MagicMock

import pytest
from concurrent import futures

import grpc

from engine.core import DataEngine
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


@pytest.fixture
def phase8_grpc_server():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine, venue_sm, mm)

    server.stop(0)


def _stub(port):
    channel = grpc.insecure_channel(f"localhost:{port}")
    return engine_pb2_grpc.DataEngineStub(channel)


# --- VenueLogin ---------------------------------------------------------

def test_venue_login_invalid_credentials_source_raises_invalid_argument(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="keyring",
                token=token,
            )
        )
    assert exc.value.code() == grpc.StatusCode.INVALID_ARGUMENT
    assert "INVALID_CREDENTIALS_SOURCE" in exc.value.details()


def test_venue_login_unknown_venue_returns_error_code(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="FOO",
            credentials_source="prompt",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "UNKNOWN_VENUE"


def test_venue_login_kabu_session_cache_returns_unsupported(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="KABU",
            credentials_source="session_cache",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "UNSUPPORTED_FOR_VENUE"


def test_venue_login_tachibana_prompt_returns_not_implemented(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="TACHIBANA",
            credentials_source="prompt",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "NOT_IMPLEMENTED"


# --- VenueLogout --------------------------------------------------------

def test_venue_logout_returns_success(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogout(engine_pb2.VenueLogoutRequest(token=token))
    assert resp.success is True
    assert resp.error_code == ""


# --- SetExecutionMode ---------------------------------------------------

def test_set_execution_mode_invalid_mode_raises_invalid_argument(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Foo", token=token))
    assert exc.value.code() == grpc.StatusCode.INVALID_ARGUMENT


def test_set_execution_mode_replay_precondition(phase8_grpc_server):
    port, token, engine, venue_sm, mm = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Replay", token=token))
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


# --- token 検証 (RED: handler 未実装) ---------------------------------------

def test_venue_login_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token="wrong-token",
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_venue_logout_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.VenueLogout(engine_pb2.VenueLogoutRequest(token="wrong-token"))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_set_execution_mode_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SetExecutionMode(
            engine_pb2.SetExecutionModeRequest(mode="LiveManual", token="wrong-token")
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_subscribe_market_data_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SubscribeMarketData(
            engine_pb2.SubscribeRequest(
                instrument_id="7203.TSE",
                channels=["bar"],
                token="wrong-token",
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_unsubscribe_market_data_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.UnsubscribeMarketData(
            engine_pb2.UnsubscribeRequest(
                instrument_id="7203.TSE", token="wrong-token"
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


@pytest.fixture
def phase8_grpc_server_with_live():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    def live_adapter_factory():
        adapter = MockVenueAdapter()
        asyncio.new_event_loop().run_until_complete(adapter.login(None))
        return adapter

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(
        token,
        engine,
        mode_manager=mm,
        venue_sm=venue_sm,
        live_adapter_factory=live_adapter_factory,
    )
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine, venue_sm, mm, servicer)

    # Teardown live components if any test spawned them.
    # 同期 API を使う: bridge/runner は servicer._live_loop (background thread)
    # に attach されているので、別 loop で asyncio.run(async版) を呼ぶと
    # "attached to a different loop" になる。
    if servicer._live_runner is not None or servicer._live_bridge is not None:
        servicer._teardown_live_components()

    server.stop(0)


def test_set_execution_mode_live_manual_spawns_live_runner(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)
    resp = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp.success is True
    assert servicer._live_runner is not None
    assert servicer._live_bridge is not None


def test_set_execution_mode_replay_teardown_live_runner(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)
    # Replay 戻しは replay_engine.replay_state in {LOADED,RUNNING,PAUSED} が precondition (mode_manager.py L24-29)
    engine._replay_state = "LOADED"

    # LiveManual で runner/bridge が立ち上がることを前提として確認
    resp_live = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_live.success is True
    assert servicer._live_runner is not None
    assert servicer._live_bridge is not None

    # Replay に戻したとき teardown が走り、参照が解放されることを期待 (RED)
    resp_replay = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="Replay", token=token)
    )
    assert resp_replay.success is True
    assert resp_replay.execution_mode == "Replay"
    assert servicer._live_runner is None, "Replay 切替時に live runner が teardown されるべき"
    assert servicer._live_bridge is None, "Replay 切替時に live bridge が teardown されるべき"


# --- Step 3.3 RED: live_adapter_factory 未設定時の LiveManual 拒否 ---------

def test_set_execution_mode_live_manual_without_adapter_factory_returns_error(
    phase8_grpc_server,
):
    """
    live_adapter_factory が None のまま LiveManual を要求された場合、
    silent に success=True を返してはならない。
    - response.success == False
    - response.error_code == "LIVE_ADAPTER_NOT_CONFIGURED"
    - response.execution_mode != "LiveManual" (遷移していない)
    RED 期待: 現実装は _start_live_components が silent return するため
    success=True, execution_mode="LiveManual" が返って失敗する。
    """
    port, token, engine, venue_sm, mm = phase8_grpc_server
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)
    resp = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp.success is False, (
        f"factory 未設定で LiveManual を許可してはならない (silent green failure): "
        f"success={resp.success}, error_code={resp.error_code!r}, "
        f"execution_mode={resp.execution_mode!r}"
    )
    assert resp.error_code == "LIVE_ADAPTER_NOT_CONFIGURED", (
        f"error_code must be LIVE_ADAPTER_NOT_CONFIGURED, got {resp.error_code!r}"
    )
    assert resp.execution_mode != "LiveManual", (
        f"execution_mode は LiveManual に遷移してはならない: {resp.execution_mode!r}"
    )


def test_subscribe_market_data_succeeds_in_live_mode(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)

    # Live mode へ遷移し runner を起動
    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True
    assert servicer._live_runner is not None

    # Subscribe を発行
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE",
            channels=["trades", "depth"],
            token=token,
        )
    )
    assert resp.success is True
    assert resp.error_code == ""

    # runner 側 aggregator に登録されていることを確認
    assert "7203.TSE" in servicer._live_runner._aggregators


def test_subscribe_market_data_rejects_without_live_runner(
    phase8_grpc_server,
):
    # Replay モード (runner なし) では precondition reject
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE", channels=["trades"], token=token
        )
    )
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


def test_unsubscribe_market_data_succeeds_after_subscribe(
    phase8_grpc_server_with_live,
):
    """Subscribe 済み instrument を Unsubscribe すると success=True を返し、
    runner._aggregators から消える。"""
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)

    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True
    assert servicer._live_runner is not None

    # Subscribe してから Unsubscribe
    resp_sub = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE",
            channels=["trades", "depth"],
            token=token,
        )
    )
    assert resp_sub.success is True
    assert "7203.TSE" in servicer._live_runner._aggregators

    resp = stub.UnsubscribeMarketData(
        engine_pb2.UnsubscribeRequest(instrument_id="7203.TSE", token=token)
    )
    assert resp.success is True
    assert resp.error_code == ""
    assert "7203.TSE" not in servicer._live_runner._aggregators


def test_unsubscribe_market_data_rejects_without_live_runner(
    phase8_grpc_server,
):
    """Replay モード (runner なし) では precondition reject。"""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.UnsubscribeMarketData(
        engine_pb2.UnsubscribeRequest(instrument_id="7203.TSE", token=token)
    )
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


def test_get_state_exposes_live_last_error_when_runner_failed(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    mock_runner = MagicMock()
    mock_runner.last_error = ConnectionError("boom")
    servicer._live_runner = mock_runner
    stub = _stub(port)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] == "ConnectionError: boom"


def test_get_state_live_last_error_is_none_when_no_error(
    phase8_grpc_server,
):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] is None
