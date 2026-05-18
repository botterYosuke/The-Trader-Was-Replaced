import pytest
from concurrent import futures

import grpc

from engine.core import DataEngine
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
        )
    )
    assert resp.success is False
    assert resp.error_code == "NOT_IMPLEMENTED"


# --- VenueLogout --------------------------------------------------------

def test_venue_logout_returns_success(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogout(engine_pb2.VenueLogoutRequest())
    assert resp.success is True
    assert resp.error_code == ""


# --- SetExecutionMode ---------------------------------------------------

def test_set_execution_mode_invalid_mode_raises_invalid_argument(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Foo"))
    assert exc.value.code() == grpc.StatusCode.INVALID_ARGUMENT


def test_set_execution_mode_replay_precondition(phase8_grpc_server):
    port, token, engine, venue_sm, mm = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Replay"))
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


def test_set_execution_mode_live_manual_succeeds_when_venue_connected(phase8_grpc_server):
    port, token, engine, venue_sm, mm = phase8_grpc_server
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)
    resp = stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="LiveManual"))
    assert resp.success is True
    assert resp.error_code == ""
    assert resp.execution_mode == "LiveManual"


# --- SubscribeMarketData / UnsubscribeMarketData ------------------------

def test_subscribe_market_data_returns_not_implemented(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(instrument_id="7203.TSE", channels=["bar"])
    )
    assert resp.success is False
    assert resp.error_code == "NOT_IMPLEMENTED"


def test_unsubscribe_market_data_returns_not_implemented(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.UnsubscribeMarketData(
        engine_pb2.UnsubscribeRequest(instrument_id="7203.TSE")
    )
    assert resp.success is False
    assert resp.error_code == "NOT_IMPLEMENTED"
