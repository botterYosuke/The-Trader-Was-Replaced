"""BackendEventBus spec (Phase 9 §3.12 / Step 0 — threadsafe fan-out)。

責務: queue.Queue ベースの threadsafe fan-out のみ。
- publish(event) で全 subscriber に同じ event を配る
- subscribe() で blocking iterator を返す（複数 subscriber が独立に消費）
- close() で全 subscriber を終端する
- subscription.close() で個別 subscriber を外す（leak 防止）

gRPC servicer が sync ThreadPool のため asyncio ではなく threading で検証する。
"""
from __future__ import annotations

import threading

import pytest

from engine.live.backend_event_bus import BackendEventBus


def test_single_subscriber_receives_published_events():
    bus: BackendEventBus[int] = BackendEventBus()
    sub = bus.subscribe()
    bus.publish(1)
    bus.publish(2)
    bus.close()
    got = list(sub)
    assert got == [1, 2]


def test_multiple_subscribers_each_receive_full_stream():
    bus: BackendEventBus[int] = BackendEventBus()
    a = bus.subscribe()
    bus.publish(1)
    b = bus.subscribe()  # 1 を見逃す
    bus.publish(2)
    bus.close()
    assert list(a) == [1, 2]
    assert list(b) == [2]


def test_close_terminates_subscriber_iterators():
    bus: BackendEventBus[int] = BackendEventBus()
    sub = bus.subscribe()
    bus.close()
    assert list(sub) == []


def test_publish_after_close_is_rejected():
    bus: BackendEventBus[int] = BackendEventBus()
    bus.close()
    with pytest.raises(RuntimeError):
        bus.publish(1)


def test_subscription_close_removes_it_from_bus():
    bus: BackendEventBus[int] = BackendEventBus()
    sub = bus.subscribe()
    sub.close()
    assert len(bus._subscribers) == 0
    # close 後の publish は残った subscriber には届かない
    bus.publish(99)  # raises しない（bus 自体は open）


def test_cross_thread_push_unblocks_waiting_consumer():
    """別 thread からの publish が、blocking 中の consumer を解放する
    （gRPC streaming handler の cross-thread push を模す）。"""
    bus: BackendEventBus[str] = BackendEventBus()
    sub = bus.subscribe()
    received: list[str] = []

    def consume():
        received.append(next(iter(sub)))

    t = threading.Thread(target=consume)
    t.start()
    bus.publish("hello")
    t.join(timeout=2.0)
    assert received == ["hello"]
