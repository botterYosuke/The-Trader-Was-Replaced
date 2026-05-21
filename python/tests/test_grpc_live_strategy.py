"""gRPC live strategy execution spec (Phase 10 Step 3).

LiveStrategyHost + RunRegistry + StrategyRegistry を gRPC 経由で mock adapter に
疎通させる:
- bad token は UNAUTHENTICATED abort（全 RPC）
- RegisterLiveStrategy: 検証成功 → strategy_id/sha256/display_name、不正パスは構造化 error
- StartLiveStrategy: LiveAuto 以外は EXECUTION_MODE_PRECONDITION、未ログインは
  VENUE_LOGIN_REQUIRED、未登録 id は UNKNOWN_STRATEGY_ID、二重起動は
  LIVE_STRATEGY_ALREADY_RUNNING
- Register→Start→Pause→Resume→Stop の lifecycle と LiveStrategyEvent push
- GetLiveStrategyStatus / ListLiveStrategies の往復

engine bridge は Step 3 placeholder（NoopLiveEngineController）。実発注はしないが
RPC 配線・state machine・RunRegistry・イベント transport の疎通を検証する。
"""
import time
from concurrent import futures
from pathlib import Path

import grpc
import pytest

from engine.core import DataEngine
from engine.live.engine_controller import NoopLiveEngineController
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.order_facade import ManualOrderFacade
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer

_STRATEGY_FILE = str(
    (Path(__file__).parent / "fixtures" / "strategies" / "fake_buy_and_hold.py").resolve()
)


@pytest.fixture
def live_strategy_server():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    # Step 3 plumbing tests inject the Noop controller: they verify the gRPC/state-machine/
    # RunRegistry/event wiring, not the real Nautilus engine bridge (covered by Step 4's
    # tests/live/test_nautilus_live_exec.py). The default servicer controller is now the
    # real NautilusLiveEngineController, which would build a kernel on StartLiveStrategy.
    servicer = GrpcDataEngineServer(
        token,
        engine,
        mode_manager=mm,
        venue_sm=venue_sm,
        engine_controller=NoopLiveEngineController(),
    )
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


def _wait_until(predicate, timeout=5.0, interval=0.02):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return bool(predicate())


def _arm_live_auto(servicer) -> MockVenueAdapter:
    """Put the servicer into a logged-in LiveAuto session backed by a mock."""
    adapter = MockVenueAdapter()
    adapter.is_logged_in = True
    servicer._order_facade = ManualOrderFacade(adapter)
    servicer.mode_manager.current_mode = "LiveAuto"
    return adapter


def _register(stub, token, **over):
    base = dict(token=token, request_id="r1", strategy_file=_STRATEGY_FILE)
    base.update(over)
    return stub.RegisterLiveStrategy(engine_pb2.RegisterLiveStrategyReq(**base))


def _start(stub, token, strategy_id, **over):
    base = dict(
        token=token,
        request_id="s1",
        strategy_id=strategy_id,
        instrument_id="7203.TSE",
        venue="MOCK",
    )
    base.update(over)
    return stub.StartLiveStrategy(engine_pb2.StartLiveStrategyReq(**base))


# --- auth -------------------------------------------------------------------

def test_all_rpcs_reject_bad_token(live_strategy_server):
    port, _token, _servicer = live_strategy_server
    stub = _stub(port)
    bad = "wrong"
    calls = [
        lambda: stub.RegisterLiveStrategy(
            engine_pb2.RegisterLiveStrategyReq(token=bad, strategy_file=_STRATEGY_FILE)
        ),
        lambda: stub.StartLiveStrategy(
            engine_pb2.StartLiveStrategyReq(token=bad, strategy_id="x")
        ),
        lambda: stub.StopLiveStrategy(engine_pb2.StopLiveStrategyReq(token=bad, run_id="x")),
        lambda: stub.PauseLiveStrategy(engine_pb2.PauseLiveStrategyReq(token=bad, run_id="x")),
        lambda: stub.ResumeLiveStrategy(engine_pb2.ResumeLiveStrategyReq(token=bad, run_id="x")),
        lambda: stub.GetLiveStrategyStatus(
            engine_pb2.GetLiveStrategyStatusReq(token=bad, run_id="x")
        ),
        lambda: stub.ListLiveStrategies(engine_pb2.ListLiveStrategiesReq(token=bad)),
    ]
    for call in calls:
        with pytest.raises(grpc.RpcError) as exc:
            call()
        assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


# --- RegisterLiveStrategy ---------------------------------------------------

def test_register_success_returns_handle(live_strategy_server):
    port, token, _servicer = live_strategy_server
    res = _register(_stub(port), token)
    assert res.success and res.error_code == ""
    assert res.strategy_id.startswith("strat-")
    assert len(res.strategy_sha256) == 64
    assert res.display_name == "FakeBuyAndHold"


def test_register_missing_file_is_structured_error(live_strategy_server):
    port, token, _servicer = live_strategy_server
    res = _register(_stub(port), token, strategy_file="/no/such/strategy.py")
    assert not res.success
    assert res.error_code == "STRATEGY_FILE_NOT_FOUND"


def test_register_hash_mismatch(live_strategy_server):
    port, token, _servicer = live_strategy_server
    res = _register(_stub(port), token, expected_sha256="deadbeef")
    assert not res.success
    assert res.error_code == "STRATEGY_HASH_MISMATCH"


# --- StartLiveStrategy preconditions ---------------------------------------

def test_start_rejected_when_not_live_auto(live_strategy_server):
    """Default Replay mode (and LiveManual) structurally rejects StartLiveStrategy."""
    port, token, servicer = live_strategy_server
    sid = _register(_stub(port), token).strategy_id
    # Replay (default)
    res = _start(_stub(port), token, sid)
    assert not res.success and res.error_code == "EXECUTION_MODE_PRECONDITION"
    # LiveManual is also not LiveAuto
    servicer.mode_manager.current_mode = "LiveManual"
    res = _start(_stub(port), token, sid)
    assert not res.success and res.error_code == "EXECUTION_MODE_PRECONDITION"


def test_start_requires_login_when_live_auto(live_strategy_server):
    port, token, servicer = live_strategy_server
    sid = _register(_stub(port), token).strategy_id
    servicer.mode_manager.current_mode = "LiveAuto"  # but no order facade armed
    res = _start(_stub(port), token, sid)
    assert not res.success and res.error_code == "VENUE_LOGIN_REQUIRED"


def test_start_unknown_strategy_id(live_strategy_server):
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    res = _start(_stub(port), token, "strat-doesnotexist")
    assert not res.success and res.error_code == "UNKNOWN_STRATEGY_ID"


# --- lifecycle + events -----------------------------------------------------

def test_start_pause_resume_stop_lifecycle(live_strategy_server):
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id

    started = _start(stub, token, sid)
    assert started.success
    run_id = started.run_id
    assert run_id
    assert started.status.status == "RUNNING"
    assert started.status.nautilus_strategy_id.startswith("LIVE-")
    assert started.status.instrument_id == "7203.TSE"

    paused = stub.PauseLiveStrategy(
        engine_pb2.PauseLiveStrategyReq(token=token, run_id=run_id)
    )
    assert paused.success and paused.status.status == "PAUSED"

    resumed = stub.ResumeLiveStrategy(
        engine_pb2.ResumeLiveStrategyReq(token=token, run_id=run_id)
    )
    assert resumed.success and resumed.status.status == "RUNNING"

    stopped = stub.StopLiveStrategy(
        engine_pb2.StopLiveStrategyReq(token=token, run_id=run_id)
    )
    assert stopped.success and stopped.status.status == "STOPPED"


def test_second_start_rejected_while_running(live_strategy_server):
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id
    first = _start(stub, token, sid)
    assert first.success
    # Same strategy + same instrument → single-run slot + duplicate both fire;
    # the single-run constraint is checked first → LIVE_STRATEGY_ALREADY_RUNNING.
    second = _start(stub, token, sid)
    assert not second.success
    assert second.error_code == "LIVE_STRATEGY_ALREADY_RUNNING"
    # After stopping, a new run can start again (slot freed).
    stub.StopLiveStrategy(engine_pb2.StopLiveStrategyReq(token=token, run_id=first.run_id))
    third = _start(stub, token, sid)
    assert third.success


def test_control_unknown_run(live_strategy_server):
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    res = stub.StopLiveStrategy(
        engine_pb2.StopLiveStrategyReq(token=token, run_id="nope")
    )
    assert not res.success and res.error_code == "UNKNOWN_RUN"


def test_double_pause_returns_structured_error_not_rpc_error(live_strategy_server):
    """不正遷移は gRPC 500 ではなく success=false / structured error_code で返る。"""
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id
    run_id = _start(stub, token, sid).run_id
    assert stub.PauseLiveStrategy(
        engine_pb2.PauseLiveStrategyReq(token=token, run_id=run_id)
    ).success
    again = stub.PauseLiveStrategy(
        engine_pb2.PauseLiveStrategyReq(token=token, run_id=run_id)
    )
    assert not again.success
    assert again.error_code == "INVALID_LIVE_STRATEGY_STATE"


def test_get_status_and_list(live_strategy_server):
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id

    # No run yet → empty list, unknown status.
    assert stub.ListLiveStrategies(
        engine_pb2.ListLiveStrategiesReq(token=token)
    ).strategies == []
    unk = stub.GetLiveStrategyStatus(
        engine_pb2.GetLiveStrategyStatusReq(token=token, run_id="x")
    )
    assert not unk.success and unk.error_code == "UNKNOWN_RUN"

    run_id = _start(stub, token, sid).run_id
    got = stub.GetLiveStrategyStatus(
        engine_pb2.GetLiveStrategyStatusReq(token=token, run_id=run_id)
    )
    assert got.success and got.status.run_id == run_id and got.status.status == "RUNNING"

    listing = stub.ListLiveStrategies(engine_pb2.ListLiveStrategiesReq(token=token))
    assert [s.run_id for s in listing.strategies] == [run_id]

    # Stopped run drops out of the active listing but is still queryable.
    stub.StopLiveStrategy(engine_pb2.StopLiveStrategyReq(token=token, run_id=run_id))
    assert stub.ListLiveStrategies(
        engine_pb2.ListLiveStrategiesReq(token=token)
    ).strategies == []
    assert stub.GetLiveStrategyStatus(
        engine_pb2.GetLiveStrategyStatusReq(token=token, run_id=run_id)
    ).status.status == "STOPPED"


def test_lifecycle_pushes_live_strategy_events(live_strategy_server):
    """LiveStrategyEvent fires on the backend stream for each transition (M8)."""
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id

    events = []

    def _drain():
        sub_stub = _stub(port)
        try:
            for ev in sub_stub.SubscribeBackendEvents(
                engine_pb2.SubscribeBackendEventsReq(token=token)
            ):
                if ev.WhichOneof("payload") == "live_strategy_event":
                    events.append(ev.live_strategy_event)
        except grpc.RpcError:
            pass

    import threading

    t = threading.Thread(target=_drain, daemon=True)
    t.start()
    time.sleep(0.2)  # let the subscription establish

    run_id = _start(stub, token, sid).run_id
    stub.PauseLiveStrategy(engine_pb2.PauseLiveStrategyReq(token=token, run_id=run_id))
    stub.StopLiveStrategy(engine_pb2.StopLiveStrategyReq(token=token, run_id=run_id))

    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline and len(events) < 3:
        time.sleep(0.02)

    statuses = [e.status for e in events]
    assert "RUNNING" in statuses
    assert "PAUSED" in statuses
    assert "STOPPED" in statuses
    assert all(e.run_id == run_id for e in events)


# --- Step 4: post-trade max_daily_loss ------------------------------------

def test_post_trade_daily_loss_stops_run_and_pushes_violation(live_strategy_server):
    """口座スナップショットの当日 P&L が max_daily_loss を割ると run を STOPPED にし
    SafetyRailViolation を push する (§2.4 post-trade)。Noop controller なので fail_run の
    teardown は no-op（実 kernel の loop 往復デッドロックは production 経路のみ）。"""
    from engine.live.order_types import AccountSnapshot

    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id
    started = stub.StartLiveStrategy(
        engine_pb2.StartLiveStrategyReq(
            token=token,
            request_id="s1",
            strategy_id=sid,
            instrument_id="7203.TSE",
            venue="MOCK",
            safety_limits=engine_pb2.SafetyLimits(max_daily_loss_jpy=100_000),
        )
    )
    assert started.success
    run_id = started.run_id

    violations = []

    def _drain():
        try:
            for ev in _stub(port).SubscribeBackendEvents(
                engine_pb2.SubscribeBackendEventsReq(token=token)
            ):
                if ev.WhichOneof("payload") == "safety_rail_violation":
                    violations.append(ev.safety_rail_violation)
        except grpc.RpcError:
            pass

    import threading

    threading.Thread(target=_drain, daemon=True).start()
    time.sleep(0.2)

    # 1st snapshot = baseline (equity 10M). 2nd = -200k P&L → breaches 100k loss cap.
    servicer._publish_account_snapshot(AccountSnapshot(cash=10_000_000.0, buying_power=10_000_000.0, positions=()))
    servicer._publish_account_snapshot(AccountSnapshot(cash=9_800_000.0, buying_power=9_800_000.0, positions=()))

    assert _wait_until(
        lambda: stub.GetLiveStrategyStatus(
            engine_pb2.GetLiveStrategyStatusReq(token=token, run_id=run_id)
        ).status.status == "STOPPED"
    )
    assert _wait_until(lambda: any(v.kind == "MAX_DAILY_LOSS" for v in violations))
    assert violations[0].run_id == run_id


def test_post_trade_eval_does_not_block_on_live_strategy_lock(live_strategy_server):
    """Finding 1 (Step 4 review): post-trade 評価は live loop thread（AccountSync callback）
    から走る。stop/fail の teardown が `_live_strategy_lock` を blocking round-trip 中ずっと
    保持しても、評価がその lock を待ってブロックしないこと（相互デッドロック回避）を確認する。"""
    from engine.live.order_types import AccountSnapshot
    import threading

    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id
    run_id = stub.StartLiveStrategy(
        engine_pb2.StartLiveStrategyReq(
            token=token, request_id="s1", strategy_id=sid,
            instrument_id="7203.TSE", venue="MOCK",
            safety_limits=engine_pb2.SafetyLimits(max_daily_loss_jpy=100_000),
        )
    ).run_id
    # baseline を確定させる。
    servicer._evaluate_post_trade_loss(
        AccountSnapshot(cash=10_000_000.0, buying_power=10_000_000.0, positions=())
    )

    # 別スレッドで `_live_strategy_lock` を保持し続ける（teardown 中の round-trip を模擬）。
    holding, release = threading.Event(), threading.Event()

    def _hold():
        with servicer._live_strategy_lock:
            holding.set()
            release.wait(5)

    threading.Thread(target=_hold, daemon=True).start()
    assert holding.wait(2), "could not acquire _live_strategy_lock in helper"

    # lock 保持中でも評価は即座に完了しなければならない（_run_rails_lock しか取らない）。
    start = time.monotonic()
    servicer._evaluate_post_trade_loss(
        AccountSnapshot(cash=9_800_000.0, buying_power=9_800_000.0, positions=())  # -200k
    )
    assert time.monotonic() - start < 1.0, "post-trade eval blocked on _live_strategy_lock"

    # lock 解放後、worker の fail_run が走り run は STOPPED に達する。
    release.set()
    assert _wait_until(
        lambda: stub.GetLiveStrategyStatus(
            engine_pb2.GetLiveStrategyStatusReq(token=token, run_id=run_id)
        ).status.status == "STOPPED"
    )


# --- Step 7 A: OrderEvent.strategy_id tagging ------------------------------


def _place(stub, token, **over):
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
    return stub.PlaceOrder(engine_pb2.PlaceOrderReq(**base))


def test_manual_order_event_tagged_manual_001(live_strategy_server):
    """手動 PlaceOrder の OrderEvent は MANUAL-001 を運ぶ（unary 応答 + backend stream）。"""
    from engine.server_grpc import MANUAL_STRATEGY_ID

    assert MANUAL_STRATEGY_ID == "MANUAL-001"
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)

    pushed = []

    def _drain():
        try:
            for ev in _stub(port).SubscribeBackendEvents(
                engine_pb2.SubscribeBackendEventsReq(token=token)
            ):
                if ev.WhichOneof("payload") == "order_event":
                    pushed.append(ev.order_event)
        except grpc.RpcError:
            pass

    import threading

    threading.Thread(target=_drain, daemon=True).start()
    time.sleep(0.2)

    res = _place(stub, token)
    assert res.success, res.error_code
    assert res.order_event.strategy_id == "MANUAL-001"
    # backend stream にも MANUAL-001 付きで届く。
    assert _wait_until(lambda: any(e.strategy_id == "MANUAL-001" for e in pushed))


def test_get_orders_and_status_tagged_manual(live_strategy_server):
    """GetOrders / GetOrderStatus の OrderEvent も MANUAL-001 を運ぶ。"""
    port, token, servicer = live_strategy_server
    adapter = _arm_live_auto(servicer)
    stub = _stub(port)
    # resting order を作る（GetOrders は非終端のみ返す）。mock の既定は FILLED（終端）なので
    # ACCEPTED を仕込んで非終端に保つ。
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)
    placed = _place(stub, token, order_type="LIMIT", price=2500.0)
    assert placed.success, placed.error_code
    oid = placed.order_event.order_id

    got = stub.GetOrderStatus(
        engine_pb2.GetOrderStatusReq(token=token, order_id=oid)
    )
    assert got.success and got.order_event.strategy_id == "MANUAL-001"

    listing = stub.GetOrders(engine_pb2.GetOrdersReq(token=token))
    assert listing.orders, "expected at least the resting order"
    assert all(o.strategy_id == "MANUAL-001" for o in listing.orders)


def test_auto_order_event_callback_tags_live_strategy_id(live_strategy_server):
    """Step 7 C: controller の on_order_event callback 経由の OrderEvent は LIVE-... を運ぶ。

    実 kernel は Noop controller では作られないため、server に注入された
    `_on_auto_order_event` を直接叩いて、push 経路が strategy_id を保持することを固定する。"""
    from engine.live.order_types import OrderEventData

    port, token, servicer = live_strategy_server
    stub = _stub(port)

    pushed = []

    def _drain():
        try:
            for ev in _stub(port).SubscribeBackendEvents(
                engine_pb2.SubscribeBackendEventsReq(token=token)
            ):
                if ev.WhichOneof("payload") == "order_event":
                    pushed.append(ev.order_event)
        except grpc.RpcError:
            pass

    import threading

    threading.Thread(target=_drain, daemon=True).start()
    time.sleep(0.2)

    ev = OrderEventData(
        order_id="O-1",
        venue_order_id="V-1",
        client_order_id="O-1",
        status="FILLED",
        filled_qty=100.0,
        avg_price=2500.0,
        ts_ms=123,
    )
    servicer._on_auto_order_event(ev, "LIVE-abc12345")
    assert _wait_until(
        lambda: any(
            e.strategy_id == "LIVE-abc12345" and e.status == "FILLED" for e in pushed
        )
    )


def test_auto_telemetry_callback_resolves_run_id_and_pushes(live_strategy_server):
    """Step 7 D: on_telemetry callback は nautilus_strategy_id を run_id に逆引きして push する。"""
    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id
    started = _start(stub, token, sid)
    assert started.success
    run_id = started.run_id
    nsid = started.status.nautilus_strategy_id  # "LIVE-..."

    telem = []

    def _drain():
        try:
            for ev in _stub(port).SubscribeBackendEvents(
                engine_pb2.SubscribeBackendEventsReq(token=token)
            ):
                if ev.WhichOneof("payload") == "live_strategy_telemetry":
                    telem.append(ev.live_strategy_telemetry)
        except grpc.RpcError:
            pass

    import threading

    threading.Thread(target=_drain, daemon=True).start()
    time.sleep(0.2)

    servicer._on_auto_telemetry(
        nsid,
        {"realized_pnl": 1234.0, "unrealized_pnl": -56.0, "order_count": 3, "fill_count": 1},
    )
    assert _wait_until(lambda: any(t.run_id == run_id for t in telem))
    t0 = next(t for t in telem if t.run_id == run_id)
    assert t0.strategy_id == nsid
    assert t0.realized_pnl == 1234.0
    assert t0.unrealized_pnl == -56.0
    assert t0.order_count == 3
    assert t0.fill_count == 1

    # 逆引き不可（未知 sid）は push しない（terminal run の遅延イベントを誤報しない）。
    before = len(telem)
    servicer._on_auto_telemetry(
        "LIVE-unknown99",
        {"realized_pnl": 0.0, "unrealized_pnl": 0.0, "order_count": 0, "fill_count": 0},
    )
    time.sleep(0.3)
    assert len(telem) == before


def test_post_trade_within_loss_limit_keeps_run_running(live_strategy_server):
    """損失が上限内なら run は RUNNING のまま（誤検知しない）。"""
    from engine.live.order_types import AccountSnapshot

    port, token, servicer = live_strategy_server
    _arm_live_auto(servicer)
    stub = _stub(port)
    sid = _register(stub, token).strategy_id
    run_id = stub.StartLiveStrategy(
        engine_pb2.StartLiveStrategyReq(
            token=token, request_id="s1", strategy_id=sid,
            instrument_id="7203.TSE", venue="MOCK",
            safety_limits=engine_pb2.SafetyLimits(max_daily_loss_jpy=100_000),
        )
    ).run_id
    servicer._publish_account_snapshot(AccountSnapshot(cash=10_000_000.0, buying_power=10_000_000.0, positions=()))
    servicer._publish_account_snapshot(AccountSnapshot(cash=9_950_000.0, buying_power=9_950_000.0, positions=()))  # -50k, within
    time.sleep(0.3)
    assert stub.GetLiveStrategyStatus(
        engine_pb2.GetLiveStrategyStatusReq(token=token, run_id=run_id)
    ).status.status == "RUNNING"
