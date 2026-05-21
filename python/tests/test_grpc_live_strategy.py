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
