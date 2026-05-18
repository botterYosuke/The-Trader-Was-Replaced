"""LiveReducerBridge spec (Phase 8 Step 2e).

責務:
- live `KlineUpdate` (pydantic, ts_ns, volume) を replay reducer の
  `KlineUpdate` (dataclass, timestamp_ms, open_time_ms) に変換する。
- `LiveEventBus` を購読し、KlineUpdate が来たら data_engine.apply_replay_event
  に `ReplayTimeUpdated -> KlineUpdate` の順で流す（replay 側の不変条件と一致）。
- `DepthUpdate` は reducer の関心外なので無視する。
- `TradesUpdate` は live_runner 内で aggregator に流すため、bus には来ない想定だが、
  bridge は安全側に倒し無視する。
"""
from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import List

from engine.live.adapter import (
    DepthLevel,
    DepthUpdate,
    KlineUpdate as LiveKlineUpdate,
)
from engine.live.event_bus import LiveEventBus
from engine.live.reducer_bridge import (
    LiveReducerBridge,
    live_kline_to_reducer_kline,
    live_kline_to_replay_time_updated,
)
from engine.reducer import (
    KlineUpdate as ReducerKlineUpdate,
    ReplayEvent,
    ReplayTimeUpdated,
)


def test_live_kline_to_reducer_kline_converts_ts_and_fields() -> None:
    live = LiveKlineUpdate(
        kind="kline",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_123_456_789,
        open=100.0, high=110.0, low=95.0, close=105.0, volume=42.0,
    )
    out = live_kline_to_reducer_kline(live)
    assert isinstance(out, ReducerKlineUpdate)
    assert out.timestamp_ms == 1_700_000_000_123
    assert out.open_time_ms == 1_700_000_000_123
    assert out.open  == 100.0
    assert out.high  == 110.0
    assert out.low   == 95.0
    assert out.close == 105.0


def test_live_kline_to_replay_time_updated_uses_same_ts_ms() -> None:
    live = LiveKlineUpdate(
        kind="kline",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_999_000_000,
        open=1.0, high=1.0, low=1.0, close=1.0, volume=1.0,
    )
    out = live_kline_to_replay_time_updated(live)
    assert isinstance(out, ReplayTimeUpdated)
    assert out.timestamp_ms == 1_700_000_000_999


@dataclass
class _RecordingDataEngine:
    applied: List[ReplayEvent] = field(default_factory=list)

    def apply_replay_event(self, event: ReplayEvent) -> None:
        self.applied.append(event)


def test_bridge_forwards_kline_as_time_then_kline_in_order() -> None:
    """Bridge は live KlineUpdate を受けると ReplayTimeUpdated -> KlineUpdate の順で
    data_engine に apply する（replay 側 §4.3 不変条件と一致）。"""
    live = LiveKlineUpdate(
        kind="kline",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_000_000_000,
        open=100.0, high=110.0, low=95.0, close=105.0, volume=2.0,
    )

    async def scenario() -> _RecordingDataEngine:
        bus = LiveEventBus()
        de = _RecordingDataEngine()
        bridge = LiveReducerBridge(bus=bus, data_engine=de)
        await bridge.start()
        # publish 1 件
        await bus.publish(live)
        # bridge が処理する余地を与える
        await asyncio.sleep(0.05)
        await bridge.stop()
        await bus.close()
        return de

    de = asyncio.run(scenario())
    assert len(de.applied) == 2
    assert isinstance(de.applied[0], ReplayTimeUpdated)
    assert isinstance(de.applied[1], ReducerKlineUpdate)
    assert de.applied[0].timestamp_ms == 1_700_000_000_000
    assert de.applied[1].timestamp_ms == 1_700_000_000_000
    assert de.applied[1].close == 105.0


def test_bridge_ignores_depth_update() -> None:
    """DepthUpdate は reducer の関心外なので bridge は何もしない。"""
    depth = DepthUpdate(
        kind="depth",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_000_000_000,
        bids=[DepthLevel(price=100.0, size=1.0)],
        asks=[DepthLevel(price=101.0, size=1.0)],
    )

    async def scenario() -> _RecordingDataEngine:
        bus = LiveEventBus()
        de = _RecordingDataEngine()
        bridge = LiveReducerBridge(bus=bus, data_engine=de)
        await bridge.start()
        await bus.publish(depth)
        await asyncio.sleep(0.05)
        await bridge.stop()
        await bus.close()
        return de

    de = asyncio.run(scenario())
    assert de.applied == []


def test_bridge_stop_is_graceful_after_bus_close() -> None:
    """bus.close() 後でも bridge.stop() はハングしない。"""

    async def scenario() -> None:
        bus = LiveEventBus()
        de = _RecordingDataEngine()
        bridge = LiveReducerBridge(bus=bus, data_engine=de)
        await bridge.start()
        await bus.close()  # bus 先 close でも bridge は綺麗に止まる
        await asyncio.wait_for(bridge.stop(), timeout=1.0)

    asyncio.run(scenario())
