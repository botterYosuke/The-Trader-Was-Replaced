"""Idle gRPC shutdown spec (Phase 9 Step 8 / §3.7)。

- `should_enable_idle_shutdown`: BACKEND_SUPERVISED=1 で無効、それ以外で有効。
- `LastRequestClock`: touch で idle がリセットされる (注入 time_source で決定論的)。
- `RequestActivityInterceptor`: 全 RPC で clock を touch し continuation を素通しする。
- `IdleShutdownMonitor`: idle が timeout を超えたら on_idle を 1 回呼ぶ / fresh では呼ばない /
  stop() で fire 前に止まる。
"""
from __future__ import annotations

import threading
import time

from engine.live.idle_shutdown import (
    IdleShutdownMonitor,
    LastRequestClock,
    RequestActivityInterceptor,
    should_enable_idle_shutdown,
)


def test_should_enable_idle_shutdown_gate() -> None:
    assert should_enable_idle_shutdown({}) is True
    assert should_enable_idle_shutdown({"BACKEND_SUPERVISED": "0"}) is True
    assert should_enable_idle_shutdown({"BACKEND_SUPERVISED": "1"}) is False


def test_last_request_clock_touch_resets_idle() -> None:
    now = [100.0]
    clock = LastRequestClock(time_source=lambda: now[0])
    assert clock.idle_seconds() == 0.0
    now[0] = 130.0
    assert clock.idle_seconds() == 30.0
    clock.touch()  # last = 130
    assert clock.idle_seconds() == 0.0
    now[0] = 145.0
    assert clock.idle_seconds() == 15.0


def test_interceptor_touches_clock_and_passes_through() -> None:
    now = [100.0]
    clock = LastRequestClock(time_source=lambda: now[0])
    now[0] = 150.0  # idle would be 50 without a touch
    interceptor = RequestActivityInterceptor(clock)

    sentinel = object()

    def continuation(_details):
        return sentinel

    result = interceptor.intercept_service(continuation, object())
    assert result is sentinel  # handler は素通し
    assert clock.idle_seconds() == 0.0  # touch 済み


class _FakeClock:
    """idle_seconds を test が直接制御する fake (LastRequestClock の duck-type)。"""

    def __init__(self, idle: float) -> None:
        self.idle = idle

    def idle_seconds(self) -> float:
        return self.idle


def _wait(pred, *, tries: int = 200, step: float = 0.005) -> None:
    for _ in range(tries):
        if pred():
            return
        time.sleep(step)


def test_monitor_fires_on_idle_once() -> None:
    fired: list[int] = []
    mon = IdleShutdownMonitor(
        _FakeClock(100.0),
        on_idle=lambda: fired.append(1),
        idle_timeout_s=1.0,
        check_interval_s=0.01,
    )
    mon.start()
    _wait(lambda: bool(fired))
    mon.stop()
    assert fired == [1]  # 1 回だけ fire (fire 後に監視終了)
    assert mon.fired is True


def test_monitor_does_not_fire_while_fresh() -> None:
    fired: list[int] = []
    mon = IdleShutdownMonitor(
        _FakeClock(0.0),  # 常に idle=0
        on_idle=lambda: fired.append(1),
        idle_timeout_s=1.0,
        check_interval_s=0.01,
    )
    mon.start()
    time.sleep(0.1)  # 数 interval 回す
    mon.stop()
    assert fired == []
    assert mon.fired is False


def test_monitor_stop_prevents_fire() -> None:
    fired: list[int] = []
    # check_interval を長めにして、stop() が最初のチェック前に効くことを確認。
    mon = IdleShutdownMonitor(
        _FakeClock(100.0),
        on_idle=lambda: fired.append(1),
        idle_timeout_s=1.0,
        check_interval_s=10.0,
    )
    mon.start()
    mon.stop()  # 最初の wait(10s) 中に stop → fire しない
    assert fired == []
    assert mon.fired is False


def test_open_subscribe_stream_keeps_clock_fresh() -> None:
    """MEDIUM-3: an open SubscribeBackendEvents stream counts as activity.

    A standalone backend whose only active client is an idle event subscriber must
    NOT self-shutdown. The handler must touch the idle clock on each periodic poll
    (not only at RPC dispatch), so the monitor never fires while the stream is open.
    """
    import threading as _threading

    from engine.core import DataEngine
    from engine.live.state_machine import VenueStateMachine
    from engine.mode_manager import ModeManager
    from engine._backend_impl import GrpcDataEngineServer

    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    clock = LastRequestClock()
    servicer = GrpcDataEngineServer(
        "tok", engine, mode_manager=mm, venue_sm=venue_sm, idle_clock=clock
    )
    # Shorten the stream heartbeat so the test runs fast; it must stay well under
    # the monitor's idle_timeout below.
    servicer._subscribe_heartbeat_s = 0.05

    class _Ctx:
        """Minimal server-streaming context: stays active, ignores callbacks."""

        def __init__(self) -> None:
            self._active = True

        def is_active(self) -> bool:
            return self._active

        def add_callback(self, cb) -> None:  # noqa: ANN001
            self._cb = cb

        def abort(self, *_a, **_k):  # pragma: no cover - good token here
            raise AssertionError("should not abort with valid token")

    from engine import _proto_compat as engine_pb2

    ctx = _Ctx()
    req = engine_pb2.SubscribeBackendEventsReq(token="tok")

    # Drive the streaming handler in a worker thread; it blocks on the empty queue
    # but must wake periodically to touch the clock.
    stop = _threading.Event()

    def _pump():
        gen = servicer.SubscribeBackendEvents(req, ctx)
        for _ in gen:  # no events published → relies on periodic poll touches
            if stop.is_set():
                break

    t = _threading.Thread(target=_pump, daemon=True)
    t.start()

    fired: list[int] = []
    mon = IdleShutdownMonitor(
        clock,
        on_idle=lambda: fired.append(1),
        idle_timeout_s=0.3,
        check_interval_s=0.02,
    )
    mon.start()
    time.sleep(0.6)  # well past idle_timeout while stream is open + idle
    mon.stop()
    ctx._active = False
    stop.set()
    servicer._backend_event_bus.close()  # unblock the pump's queue.get
    t.join(timeout=2.0)

    assert fired == [], "idle monitor fired while an event stream was open"


def test_monitor_stop_is_idempotent_and_thread_exits() -> None:
    mon = IdleShutdownMonitor(
        _FakeClock(0.0), on_idle=lambda: None, check_interval_s=0.01
    )
    mon.start()
    mon.stop()
    mon.stop()  # 二重 stop は安全
    # 監視 thread は join 済みで残らない。
    assert all(
        t.name != "idle_shutdown_monitor" or not t.is_alive()
        for t in threading.enumerate()
    )
