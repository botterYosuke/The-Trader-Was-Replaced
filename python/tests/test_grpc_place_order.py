"""gRPC PlaceOrder / CancelOrder / GetOrderStatus spec (Phase 9 Step 2).

軽量 facade 経由の手動発注 RPC を mock adapter で疎通確認する:
- Replay モードでの write reject (EXECUTION_MODE_PRECONDITION)
- live runner 未起動での VENUE_LOGIN_REQUIRED
- 発注成功 → unary response の OrderEvent + SubscribeBackendEvents への push
- 取消 / GetOrderStatus の往復
- bad token は UNAUTHENTICATED
"""
import time

import grpc
import pytest
from concurrent import futures

from engine.core import DataEngine
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.order_facade import ManualOrderFacade
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


@pytest.fixture
def order_server():
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

    # Stop the live loop thread armed by _ensure_live_loop (if any) to avoid leaks.
    loop = servicer._live_loop
    if loop is not None and loop.is_running():
        loop.call_soon_threadsafe(loop.stop)
    server.stop(0)


def _stub(port):
    channel = grpc.insecure_channel(f"localhost:{port}")
    return engine_pb2_grpc.DataEngineStub(channel)


def _arm_live(servicer) -> MockVenueAdapter:
    """Put the servicer into a LiveManual session backed by a logged-in mock."""
    adapter = MockVenueAdapter()
    adapter.is_logged_in = True
    servicer._order_facade = ManualOrderFacade(adapter)
    servicer.mode_manager.current_mode = "LiveManual"
    return adapter


def _wait_until(predicate, timeout=5.0, interval=0.01):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return bool(predicate())


def _place_req(token, **over):
    base = dict(
        token=token,
        venue="MOCK",
        instrument_id="7203.TSE",
        side="BUY",
        qty=100.0,
        order_type="MARKET",
        time_in_force="DAY",
    )
    base.update(over)
    return engine_pb2.PlaceOrderReq(**base)


# --- precondition rejects --------------------------------------------------

def test_place_order_rejected_in_replay_mode(order_server):
    """Default Replay mode structurally rejects PlaceOrder."""
    port, token, _servicer = order_server
    stub = _stub(port)
    res = stub.PlaceOrder(_place_req(token))
    assert res.success is False
    assert res.error_code == "EXECUTION_MODE_PRECONDITION"


def test_place_order_requires_live_session(order_server):
    """LiveManual but no runner/facade → VENUE_LOGIN_REQUIRED."""
    port, token, servicer = order_server
    servicer.mode_manager.current_mode = "LiveManual"  # facade still None
    stub = _stub(port)
    res = stub.PlaceOrder(_place_req(token))
    assert res.success is False
    assert res.error_code == "VENUE_LOGIN_REQUIRED"


def test_place_order_rejects_bad_token(order_server):
    port, _token, _servicer = order_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.PlaceOrder(_place_req("wrong-token"))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


# --- success + push --------------------------------------------------------

def test_place_order_success_returns_and_pushes_event(order_server):
    """LiveManual: PlaceOrder fills via mock, returns OrderEvent inline AND pushes
    the same event on SubscribeBackendEvents."""
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)

    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token), timeout=5.0
    )
    bus = servicer._backend_event_bus
    assert _wait_until(lambda: bus.subscriber_count() >= 1), "stream never registered"

    res = stub.PlaceOrder(_place_req(token, order_type="LIMIT", price=2500.0))
    assert res.success is True
    assert res.error_code == ""
    assert res.order_event.status == "FILLED"
    assert res.order_event.filled_qty == 100.0
    assert res.order_event.client_order_id

    received = next(iter(stream))
    assert received.WhichOneof("payload") == "order_event"
    assert received.order_event.client_order_id == res.order_event.client_order_id
    assert received.order_event.status == "FILLED"

    stream.cancel()


def test_place_order_rejected_outcome_is_success_with_rejected_status(order_server):
    """A venue REJECTED outcome is RPC-success with order_event.status=REJECTED."""
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    adapter.set_next_order_outcome(status="REJECTED", reject_reason="margin")
    stub = _stub(port)

    res = stub.PlaceOrder(_place_req(token))
    assert res.success is True
    assert res.order_event.status == "REJECTED"
    assert res.order_event.filled_qty == 0.0


def test_place_order_invalid_params_returns_error_code(order_server):
    """Facade validation surfaces as success=False + error_code (no abort)."""
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)
    res = stub.PlaceOrder(_place_req(token, side="HOLD"))
    assert res.success is False
    assert res.error_code == "INVALID_SIDE"


# --- cancel ----------------------------------------------------------------

def test_cancel_order_success(order_server):
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)

    placed = stub.PlaceOrder(_place_req(token))
    assert placed.success is True
    coid = placed.order_event.client_order_id

    res = stub.CancelOrder(
        engine_pb2.CancelOrderReq(token=token, venue="MOCK", order_id=coid)
    )
    assert res.success is True
    assert res.order_event.status == "CANCELED"
    assert res.order_event.client_order_id == coid


def test_cancel_order_unknown_id(order_server):
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)
    res = stub.CancelOrder(
        engine_pb2.CancelOrderReq(token=token, venue="MOCK", order_id="nope")
    )
    assert res.success is False
    assert res.error_code == "UNKNOWN_ORDER_ID"


def test_cancel_order_rejected_in_replay_mode(order_server):
    port, token, _servicer = order_server
    stub = _stub(port)
    res = stub.CancelOrder(
        engine_pb2.CancelOrderReq(token=token, venue="MOCK", order_id="x")
    )
    assert res.success is False
    assert res.error_code == "EXECUTION_MODE_PRECONDITION"


# --- get status ------------------------------------------------------------

def test_get_order_status_roundtrip(order_server):
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)

    placed = stub.PlaceOrder(_place_req(token))
    coid = placed.order_event.client_order_id

    res = stub.GetOrderStatus(
        engine_pb2.GetOrderStatusReq(token=token, venue="MOCK", order_id=coid)
    )
    assert res.success is True
    assert res.order_event.client_order_id == coid
    assert res.order_event.status == "FILLED"


def test_get_order_status_unknown(order_server):
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)
    res = stub.GetOrderStatus(
        engine_pb2.GetOrderStatusReq(token=token, venue="MOCK", order_id="nope")
    )
    assert res.success is False
    assert res.error_code == "UNKNOWN_ORDER_ID"


def test_get_order_status_no_live_session(order_server):
    """Read RPC is not mode-rejected, but reports NO_LIVE_SESSION when idle."""
    port, token, _servicer = order_server
    stub = _stub(port)
    res = stub.GetOrderStatus(
        engine_pb2.GetOrderStatusReq(token=token, venue="MOCK", order_id="x")
    )
    assert res.success is False
    assert res.error_code == "NO_LIVE_SESSION"
