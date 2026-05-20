import grpc
import pytest
from concurrent import futures

from engine.core import DataEngine
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


@pytest.fixture
def submit_secret_server():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    servicer = GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, servicer)

    server.stop(0)


def _stub(port):
    channel = grpc.insecure_channel(f"localhost:{port}")
    return engine_pb2_grpc.DataEngineStub(channel)


def test_submit_secret_resolves_request_and_stores(submit_secret_server):
    """Valid token + known request_id: success, no error, secret stored in vault."""
    port, token, servicer = submit_secret_server
    stub = _stub(port)

    rid = servicer._secret_vault.create_request("tachibana", "new_order")

    res = stub.SubmitSecret(
        engine_pb2.SubmitSecretReq(token=token, request_id=rid, secret="1234")
    )

    assert res.success is True
    assert res.error_code == ""
    assert servicer._secret_vault.get("tachibana", "new_order") == "1234"


def test_submit_secret_unknown_request_id_fails_softly(submit_secret_server):
    """Unknown request_id: success=False, error_code=UNKNOWN_REQUEST_ID, no abort."""
    port, token, servicer = submit_secret_server
    stub = _stub(port)

    res = stub.SubmitSecret(
        engine_pb2.SubmitSecretReq(
            token=token, request_id="does-not-exist", secret="1234"
        )
    )

    assert res.success is False
    assert res.error_code == "UNKNOWN_REQUEST_ID"


def test_submit_secret_rejects_bad_token(submit_secret_server):
    """Bad token aborts with UNAUTHENTICATED."""
    port, _token, _servicer = submit_secret_server
    stub = _stub(port)

    with pytest.raises(grpc.RpcError) as exc:
        stub.SubmitSecret(
            engine_pb2.SubmitSecretReq(
                token="wrong-token", request_id="x", secret="1234"
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED
