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
