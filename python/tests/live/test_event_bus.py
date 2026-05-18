"""LiveEventBus spec (Phase 8 §3.3 — adapter → consumer fan-out)。

責務: asyncio Queue ベースの fan-out のみ。
- publish(event) で全 subscriber Queue に同じ event を配る
- subscribe() で新しい AsyncIterator を返す（複数 subscriber が独立に消費）
- close() で全 subscriber に終端を通知し、AsyncIterator が綺麗に止まる

pub/sub の topic / filtering は持たない（live_runner 側の責務）。
"""
from __future__ import annotations

import asyncio

import pytest

from engine.live.adapter import KlineUpdate, TradesUpdate
from engine.live.event_bus import LiveEventBus


def _kline(ts_ns: int) -> KlineUpdate:
    return KlineUpdate(
        kind="kline",
        instrument_id="7203.TSE",
        ts_ns=ts_ns,
        open=1.0, high=1.0, low=1.0, close=1.0, volume=1.0,
    )


def test_single_subscriber_receives_published_events():
    async def scenario():
        bus = LiveEventBus()
        sub = bus.subscribe()
        await bus.publish(_kline(1))
        await bus.publish(_kline(2))
        await bus.close()
        got = [ev async for ev in sub]
        return got

    got = asyncio.run(scenario())
    assert [ev.ts_ns for ev in got] == [1, 2]


def test_multiple_subscribers_each_receive_full_stream():
    """fan-out: 後から subscribe した consumer も以後の publish を全件受け取る。
    publish 前に subscribe された consumer は全件、後から subscribe した
    consumer は subscribe 以降の event のみ受け取る。
    """
    async def scenario():
        bus = LiveEventBus()
        a = bus.subscribe()
        await bus.publish(_kline(1))
        b = bus.subscribe()  # 1 を見逃す
        await bus.publish(_kline(2))
        await bus.close()
        return [ev async for ev in a], [ev async for ev in b]

    got_a, got_b = asyncio.run(scenario())
    assert [ev.ts_ns for ev in got_a] == [1, 2]
    assert [ev.ts_ns for ev in got_b] == [2]


def test_close_terminates_subscriber_iterators():
    async def scenario():
        bus = LiveEventBus()
        sub = bus.subscribe()
        await bus.close()
        return [ev async for ev in sub]

    got = asyncio.run(scenario())
    assert got == []


def test_publish_after_close_is_rejected():
    async def scenario():
        bus = LiveEventBus()
        await bus.close()
        with pytest.raises(RuntimeError):
            await bus.publish(_kline(1))

    asyncio.run(scenario())


def test_accepts_any_live_event_variant():
    """LiveEvent union の全 variant が publish できる（型限定しない）。"""
    async def scenario():
        bus = LiveEventBus()
        sub = bus.subscribe()
        await bus.publish(_kline(1))
        await bus.publish(TradesUpdate(
            kind="trades", instrument_id="7203.TSE",
            ts_ns=2, price=1.0, size=1.0, aggressor_side="buy",
        ))
        await bus.close()
        return [ev.kind async for ev in sub]

    kinds = asyncio.run(scenario())
    assert kinds == ["kline", "trades"]
