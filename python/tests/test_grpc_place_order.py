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
    adapter = _arm_live(servicer)
    stub = _stub(port)

    # Place a *working* order (ACCEPTED, no fills) — only non-terminal orders are
    # cancelable. A FILLED order would now be rejected with ORDER_NOT_CANCELABLE.
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token))
    assert placed.success is True
    coid = placed.order_event.client_order_id

    res = stub.CancelOrder(
        engine_pb2.CancelOrderReq(token=token, venue="MOCK", order_id=coid)
    )
    assert res.success is True
    assert res.order_event.status == "CANCELED"
    assert res.order_event.client_order_id == coid


def test_cancel_order_terminal_is_rejected(order_server):
    """A FILLED (terminal) order cannot be canceled — ORDER_NOT_CANCELABLE."""
    port, token, servicer = order_server
    _arm_live(servicer)  # mock default place outcome is FILLED
    stub = _stub(port)

    placed = stub.PlaceOrder(_place_req(token))
    assert placed.order_event.status == "FILLED"
    coid = placed.order_event.client_order_id

    res = stub.CancelOrder(
        engine_pb2.CancelOrderReq(token=token, venue="MOCK", order_id=coid)
    )
    assert res.success is False
    assert res.error_code == "ORDER_NOT_CANCELABLE"


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


# --- get orders (§3.8 reconcile) -------------------------------------------

def test_get_orders_returns_working_orders(order_server):
    """GetOrders は稼働中（非終端）注文を返す（再起動後の reconcile primitive）。"""
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token))
    coid = placed.order_event.client_order_id

    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True
    assert [o.client_order_id for o in res.orders] == [coid]
    assert res.orders[0].status == "ACCEPTED"


def test_get_orders_returns_full_order_rows(order_server):
    """issue #29 Slice3a: GetOrders の OrderEvent は symbol/side/qty/price を載せる。

    UI が接続/再起動後に完全な注文行（銘柄・売買・数量・指値）を seed できるよう、
    稼働中注文の proto OrderEvent に静的属性が含まれること。LIMIT は price が set、
    MARKET は price 未設定（HasField=False）。
    """
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    stub.PlaceOrder(
        _place_req(
            token,
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="LIMIT",
            price=2500.0,
        )
    )

    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True
    assert len(res.orders) == 1
    row = res.orders[0]
    assert row.symbol == "7203.TSE"
    assert row.side == "BUY"
    assert row.qty == 100.0
    assert row.HasField("price") is True
    assert row.price == 2500.0


def test_get_orders_market_row_has_no_price(order_server):
    """issue #29 Slice3a: MARKET の稼働中注文行は price 未設定（UI は MKT 表示）。"""
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    stub.PlaceOrder(_place_req(token, side="SELL", qty=300.0, order_type="MARKET"))

    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True
    row = res.orders[0]
    assert row.side == "SELL"
    assert row.qty == 300.0
    assert row.HasField("price") is False


def test_get_orders_excludes_terminal(order_server):
    """終端注文（FILLED 等）は稼働中ではないので返さない。"""
    port, token, servicer = order_server
    _arm_live(servicer)  # default outcome is FILLED (terminal)
    stub = _stub(port)

    stub.PlaceOrder(_place_req(token))
    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True
    assert list(res.orders) == []


def test_get_orders_empty_when_no_orders(order_server):
    """live session はあるが注文ゼロ（= 再起動直後の fresh backend）→ success + 空。"""
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)
    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True
    assert list(res.orders) == []


def test_get_orders_no_live_session(order_server):
    """live session 無し（facade None）→ NO_LIVE_SESSION（read RPC は mode reject しない）。"""
    port, token, _servicer = order_server
    stub = _stub(port)
    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is False
    assert res.error_code == "NO_LIVE_SESSION"


def test_get_orders_bad_token(order_server):
    port, _token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.GetOrders(engine_pb2.GetOrdersReq(token="wrong", venue="MOCK"))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_get_orders_merges_venue_working_orders(order_server):
    """Slice 3b: GetOrders は facade 注文に加え venue.fetch_working_orders() もマージする。

    facade に存在しない venue_order_id を持つ order は GetOrders に含まれる。
    """
    from engine.live.order_types import OrderEventData

    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    # facade に 1 件（ACCEPTED、venue_order_id は空 = mock 既定）
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token, instrument_id="7203.TSE", side="BUY",
                                        qty=100.0, order_type="LIMIT", price=2500.0))
    facade_coid = placed.order_event.client_order_id

    # venue から 1 件（facade には存在しない venue_order_id）
    venue_seed = [
        OrderEventData(
            order_id="v999", venue_order_id="v999", client_order_id="",
            status="ACCEPTED", filled_qty=0.0, avg_price=0.0, ts_ms=0,
            symbol="6758.TSE", side="SELL", qty=50.0, price=None,
        ),
    ]

    async def _fake_fetch_working_orders():
        return venue_seed

    adapter.fetch_working_orders = _fake_fetch_working_orders

    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True
    assert len(res.orders) == 2, "facade 1件 + venue 1件"
    coids = {o.client_order_id for o in res.orders}
    assert facade_coid in coids, "facade 注文は含まれる"
    v999 = next(o for o in res.orders if o.venue_order_id == "v999")
    assert v999.symbol == "6758.TSE"
    assert v999.side == "SELL"


def test_get_orders_surfaces_venue_fetch_timeout(order_server):
    """Medium-4: venue fetch_working_orders が timeout したら error_code を立てる。

    facade 注文は partial で返しつつ (success=True 維持)、
    error_code="VENUE_ORDERS_TIMEOUT" でユーザーに欠落を surface する。
    """
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    # facade に 1 件仕込む（partial で残ることを確認するため）
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token, instrument_id="7203.TSE", side="BUY",
                                        qty=100.0, order_type="LIMIT", price=2500.0))
    facade_coid = placed.order_event.client_order_id

    # venue fetch が timeout する
    async def _timeout_fetch_working_orders():
        raise futures.TimeoutError()

    adapter.fetch_working_orders = _timeout_fetch_working_orders

    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True, "facade 注文は partial で返す"
    assert res.error_code == "VENUE_ORDERS_TIMEOUT"
    assert len(res.orders) >= 1, "facade 注文は欠落しない"
    coids = {o.client_order_id for o in res.orders}
    assert facade_coid in coids


def test_get_orders_surfaces_non_timeout_fetch_failure(order_server):
    """Medium-C: venue fetch_working_orders が timeout 以外で失敗しても error_code を立てる。

    HTTP/auth/parse 失敗（generic except 経路）でも facade 注文は partial で
    返しつつ (success=True 維持)、error_code="VENUE_ORDERS_FETCH_FAILED" で
    欠落を surface する。timeout 分岐と同じ扱い。
    """
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    # facade に 1 件仕込む（partial で残ることを確認するため）
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token, instrument_id="7203.TSE", side="BUY",
                                        qty=100.0, order_type="LIMIT", price=2500.0))
    facade_coid = placed.order_event.client_order_id

    # venue fetch が timeout 以外の一般例外で失敗する
    async def _failing_fetch_working_orders():
        raise RuntimeError("HTTP 500 from venue")

    adapter.fetch_working_orders = _failing_fetch_working_orders

    res = stub.GetOrders(engine_pb2.GetOrdersReq(token=token, venue="MOCK"))
    assert res.success is True, "facade 注文は partial で返す"
    assert res.error_code == "VENUE_ORDERS_FETCH_FAILED"
    assert len(res.orders) >= 1, "facade 注文は欠落しない"
    coids = {o.client_order_id for o in res.orders}
    assert facade_coid in coids


# --- modify ----------------------------------------------------------------


def test_modify_order_rejected_in_replay_mode(order_server):
    """Default Replay mode structurally rejects ModifyOrder."""
    port, token, _servicer = order_server
    stub = _stub(port)
    res = stub.ModifyOrder(
        engine_pb2.ModifyOrderReq(token=token, venue="MOCK", order_id="x", new_price=10.0)
    )
    assert res.success is False
    assert res.error_code == "EXECUTION_MODE_PRECONDITION"


def test_modify_order_requires_live_session(order_server):
    """LiveManual but no facade → VENUE_LOGIN_REQUIRED."""
    port, token, servicer = order_server
    servicer.mode_manager.current_mode = "LiveManual"  # facade still None
    stub = _stub(port)
    res = stub.ModifyOrder(
        engine_pb2.ModifyOrderReq(token=token, venue="MOCK", order_id="x", new_price=10.0)
    )
    assert res.success is False
    assert res.error_code == "VENUE_LOGIN_REQUIRED"


def test_modify_order_rejects_bad_token(order_server):
    port, _token, _servicer = order_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.ModifyOrder(
            engine_pb2.ModifyOrderReq(token="wrong", venue="MOCK", order_id="x", new_price=1.0)
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_modify_order_unknown_id(order_server):
    """Facade UNKNOWN_ORDER_ID surfaces as success=False + error_code (no abort)."""
    port, token, servicer = order_server
    _arm_live(servicer)
    stub = _stub(port)
    res = stub.ModifyOrder(
        engine_pb2.ModifyOrderReq(token=token, venue="MOCK", order_id="nope", new_price=10.0)
    )
    assert res.success is False
    assert res.error_code == "UNKNOWN_ORDER_ID"


def test_modify_order_nothing_to_modify(order_server):
    """Neither new_price nor new_qty set → NOTHING_TO_MODIFY."""
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token))
    coid = placed.order_event.client_order_id
    res = stub.ModifyOrder(
        engine_pb2.ModifyOrderReq(token=token, venue="MOCK", order_id=coid)
    )
    assert res.success is False
    assert res.error_code == "NOTHING_TO_MODIFY"


def test_modify_order_success_returns_and_pushes_event(order_server):
    """LiveManual: ModifyOrder on a working order returns ACCEPTED inline AND pushes."""
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token), timeout=5.0
    )
    bus = servicer._backend_event_bus
    assert _wait_until(lambda: bus.subscriber_count() >= 1), "stream never registered"

    # working (non-terminal) order
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token))
    coid = placed.order_event.client_order_id

    res = stub.ModifyOrder(
        engine_pb2.ModifyOrderReq(token=token, venue="MOCK", order_id=coid, new_price=2600.0)
    )
    assert res.success is True
    assert res.error_code == ""
    assert res.order_event.status == "ACCEPTED"
    assert res.order_event.client_order_id == coid

    # Both place and modify push to the stream. Drain the two events and assert
    # the modify ACCEPTED for our coid is among them (order_event payload).
    stream_it = iter(stream)
    seen = [next(stream_it), next(stream_it)]
    assert any(
        ev.WhichOneof("payload") == "order_event"
        and ev.order_event.client_order_id == coid
        and ev.order_event.status == "ACCEPTED"
        for ev in seen
    )
    stream.cancel()


def test_modify_order_rejected_outcome_is_error_code(order_server):
    """A venue REJECTED modify is RPC success=False with MODIFY_REJECTED."""
    port, token, servicer = order_server
    adapter = _arm_live(servicer)
    stub = _stub(port)

    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    placed = stub.PlaceOrder(_place_req(token))
    coid = placed.order_event.client_order_id

    adapter.set_next_modify_outcome(status="REJECTED", reject_reason="too late")
    res = stub.ModifyOrder(
        engine_pb2.ModifyOrderReq(token=token, venue="MOCK", order_id=coid, new_qty=50.0)
    )
    assert res.success is False
    assert res.error_code == "MODIFY_REJECTED"
