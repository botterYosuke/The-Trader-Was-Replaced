import threading

import grpc
import pytest
from concurrent import futures

from engine.core import DataEngine
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


@pytest.fixture
def backend_events_server():
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


def test_subscribe_backend_events_yields_pushed_event(backend_events_server):
    """A BackendEvent pushed to the servicer is delivered on the open stream."""
    port, token, servicer = backend_events_server
    stub = _stub(port)

    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token)
    )

    pushed = engine_pb2.BackendEvent(
        venue_logout_detected=engine_pb2.VenueLogoutDetected(venue="KABU")
    )

    def _push():
        # Give the server-side handler a moment to register its consumer
        # before the first event is published.
        import time
        time.sleep(0.1)
        servicer.publish_backend_event(pushed)

    pusher = threading.Thread(target=_push, daemon=True)
    pusher.start()

    received = next(iter(stream))
    stream.cancel()
    pusher.join(timeout=2.0)

    assert received.WhichOneof("payload") == "venue_logout_detected"
    assert received.venue_logout_detected.venue == "KABU"


def test_subscribe_backend_events_rejects_bad_token(backend_events_server):
    """Bad token aborts the stream with UNAUTHENTICATED before any event."""
    port, _token, _servicer = backend_events_server
    stub = _stub(port)

    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token="wrong-token")
    )
    with pytest.raises(grpc.RpcError) as exc:
        next(iter(stream))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED
