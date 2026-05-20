"""gRPC Step 5 wiring: 第二暗証番号の都度収集 round-trip + OrderEvent push。

Tachibana の発注は「発注呼び出しの内側」で SecretRequired を push し、UI の
SubmitSecret RPC で解決する (SecondSecretResolver + SecretVault、計画 §1.3)。
本テストは server_grpc の結線 (publish_secret_required / publish_order_event /
order write RPC の timeout & SECRET_TIMEOUT 経路) を mock adapter ではなく
secret 経路を持つフェイク adapter で検証する。
"""
import threading
import time
from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.live.order_facade import ManualOrderFacade
from engine.live.order_types import OrderResult
from engine.live.secret_provider import SecondSecretResolver
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


class _SecretingAdapter:
    """secret 経路を踏む最小フェイク adapter (Tachibana の代役)。"""

    venue_id = "TACHIBANA"

    def __init__(self):
        self.is_logged_in = True
        self._resolver = None
        self._on_order_event = None
        self.received_secret = None
        self._n = 0

    def set_execution_hooks(self, *, secret_resolver, on_order_event):
        self._resolver = secret_resolver
        self._on_order_event = on_order_event

    async def submit_order(self, *, venue, instrument_id, side, qty, price,
                           order_type, time_in_force, **extra) -> OrderResult:
        # 発注の内側で第二暗証番号を都度収集する (Tachibana 本実装と同じ流れ)。
        self.received_secret = await self._resolver.resolve("TACHIBANA", "new_order")
        self._n += 1
        return OrderResult(
            status="ACCEPTED", filled_qty=0.0, avg_price=None,
            client_order_id=f"cid-{self._n}",
        )


@pytest.fixture
def order_server():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=8))
    servicer = GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, servicer)

    loop = servicer._live_loop
    if loop is not None and loop.is_running():
        loop.call_soon_threadsafe(loop.stop)
    server.stop(0)


def _stub(port):
    return engine_pb2_grpc.DataEngineStub(grpc.insecure_channel(f"localhost:{port}"))


def _wait_until(predicate, timeout=5.0, interval=0.01):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return bool(predicate())


def _arm_secreting(servicer, *, timeout=30.0):
    """LiveManual + secret 経路フェイク adapter を本番と同じ結線で arm する。"""
    adapter = _SecretingAdapter()
    servicer._order_facade = ManualOrderFacade(adapter)
    servicer.mode_manager.current_mode = "LiveManual"
    resolver = SecondSecretResolver(
        servicer._secret_vault, servicer._publish_secret_required, timeout=timeout
    )
    adapter.set_execution_hooks(
        secret_resolver=resolver, on_order_event=servicer._publish_order_event
    )
    return adapter


def _place_req(token):
    return engine_pb2.PlaceOrderReq(
        token=token, venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, order_type="MARKET", time_in_force="DAY",
    )


# --- publish helpers -------------------------------------------------------


def test_publish_secret_required_emits_event(order_server):
    port, token, servicer = order_server
    stub = _stub(port)
    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token), timeout=5.0
    )
    bus = servicer._backend_event_bus
    assert _wait_until(lambda: bus.subscriber_count() >= 1)

    servicer._publish_secret_required("rid-1", "TACHIBANA", "second_secret", "new_order")

    ev = next(iter(stream))
    assert ev.WhichOneof("payload") == "secret_required"
    assert ev.secret_required.request_id == "rid-1"
    assert ev.secret_required.venue == "TACHIBANA"
    assert ev.secret_required.kind == "second_secret"
    assert ev.secret_required.purpose == "new_order"
    stream.cancel()


def test_publish_order_event_emits_event(order_server):
    from engine.live.order_types import OrderEventData
    port, token, servicer = order_server
    stub = _stub(port)
    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token), timeout=5.0
    )
    assert _wait_until(lambda: servicer._backend_event_bus.subscriber_count() >= 1)

    servicer._publish_order_event(OrderEventData(
        order_id="cid-1", venue_order_id="9000015", client_order_id="cid-1",
        status="FILLED", filled_qty=100.0, avg_price=2430.0, ts_ms=1_700_000_000_000,
    ))

    ev = next(iter(stream))
    assert ev.WhichOneof("payload") == "order_event"
    assert ev.order_event.venue_order_id == "9000015"
    assert ev.order_event.status == "FILLED"
    stream.cancel()


# --- secret round-trip -----------------------------------------------------


def test_place_order_collects_second_secret_then_succeeds(order_server):
    """PlaceOrder が SecretRequired を push → SubmitSecret で解決 → ACCEPTED。"""
    port, token, servicer = order_server
    adapter = _arm_secreting(servicer)
    stub = _stub(port)

    stream = stub.SubscribeBackendEvents(
        engine_pb2.SubscribeBackendEventsReq(token=token), timeout=10.0
    )
    assert _wait_until(lambda: servicer._backend_event_bus.subscriber_count() >= 1)

    # PlaceOrder は secret 入力待ちでブロックするため別スレッドで発射する。
    box: dict = {}

    def call():
        box["res"] = stub.PlaceOrder(_place_req(token))

    t = threading.Thread(target=call)
    t.start()

    # 発注の内側から SecretRequired が push される。
    ev = next(iter(stream))
    assert ev.WhichOneof("payload") == "secret_required"
    request_id = ev.secret_required.request_id
    assert ev.secret_required.purpose == "new_order"

    # UI 応答 (別の worker thread で submit → cross-thread に live loop を起こす)。
    sres = stub.SubmitSecret(
        engine_pb2.SubmitSecretReq(token=token, request_id=request_id, secret="pswd")
    )
    assert sres.success is True

    t.join(timeout=10.0)
    assert "res" in box, "PlaceOrder did not return"
    assert box["res"].success is True
    assert box["res"].order_event.status == "ACCEPTED"
    assert adapter.received_secret == "pswd"
    stream.cancel()


def test_place_order_secret_timeout_returns_error_code(order_server):
    """SubmitSecret が来なければ SECRET_TIMEOUT を返す (注文は未送信)。"""
    port, token, servicer = order_server
    adapter = _arm_secreting(servicer, timeout=0.3)
    stub = _stub(port)

    res = stub.PlaceOrder(_place_req(token))
    assert res.success is False
    assert res.error_code == "SECRET_TIMEOUT"
    assert adapter.received_secret is None  # secret 未取得 → 発注に到達しない
