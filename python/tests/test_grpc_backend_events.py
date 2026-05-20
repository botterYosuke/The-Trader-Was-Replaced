import time

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


def _wait_until(predicate, timeout=5.0, interval=0.01):
    """Poll `predicate` until true or `timeout` elapses; returns its last value."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return bool(predicate())


def test_subscribe_backend_events_yields_pushed_event(backend_events_server):
    """A BackendEvent pushed to the servicer is delivered on the open stream."""
    port, token, servicer = backend_events_server
    stub = _stub(port)

    # 5s deadline so a lost-event race fails fast instead of hanging the suite.
    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token), timeout=5.0
    )

    pushed = engine_pb2.BackendEvent(
        venue_logout_detected=engine_pb2.VenueLogoutDetected(venue="KABU")
    )

    # The bus does not replay past events, so publishing before the server-side
    # handler has registered its subscription would silently drop the event.
    # Wait for the subscription to appear (deterministic) rather than sleeping
    # and hoping the handler won the race.
    bus = servicer._backend_event_bus
    assert _wait_until(lambda: bus.subscriber_count() >= 1), "stream never registered"
    servicer.publish_backend_event(pushed)

    received = next(iter(stream))
    assert received.WhichOneof("payload") == "venue_logout_detected"
    assert received.venue_logout_detected.venue == "KABU"

    # Cancelling the RPC must remove the subscription from the bus — this is the
    # context.add_callback(sub.close) leak/hang guard. Verify it actually fires.
    stream.cancel()
    assert _wait_until(
        lambda: bus.subscriber_count() == 0
    ), "subscription leaked after cancel"


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
