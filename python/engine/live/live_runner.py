"""LiveRunner — adapter → aggregator → event_bus pipeline (Phase 8 §3, Step 1).

責務 (Step 1 スコープ):
- subscribe(instrument_id): adapter に {"trades"} を購読し、
  内部に TickBarAggregator を 1 個作る。
- start(): adapter.events() を消費する background task を起動。
  TradesUpdate を aggregator.on_tick に流し、bar が確定したら
  self.bus.publish(KlineUpdate) する。
- stop(): background task を cancel して await し、bus を close。

Step スコープ外:
- reducer 接続 / Nautilus 型変換
- depth / kline 直 pass-through
- 複数 interval
"""
from __future__ import annotations

import asyncio
from typing import Optional

from engine.live.adapter import (
    InstrumentId,
    LiveVenueAdapter,
    TradesUpdate,
)
from engine.live.aggregator import TickBarAggregator
from engine.live.event_bus import LiveEventBus


class LiveRunner:
    def __init__(self, adapter: LiveVenueAdapter, interval_ns: int) -> None:
        if interval_ns <= 0:
            raise ValueError("interval_ns must be positive")
        self._adapter = adapter
        self._interval_ns = interval_ns
        self._aggregators: dict[InstrumentId, TickBarAggregator] = {}
        self.bus: LiveEventBus = LiveEventBus()
        self._task: Optional[asyncio.Task[None]] = None

    async def subscribe(self, instrument_id: InstrumentId) -> None:
        await self._adapter.subscribe(instrument_id, {"trades"})
        self._aggregators[instrument_id] = TickBarAggregator(
            instrument_id=instrument_id,
            interval_ns=self._interval_ns,
        )

    async def start(self) -> None:
        if self._task is not None:
            return
        self._task = asyncio.create_task(self._run())

    async def _run(self) -> None:
        try:
            async for evt in self._adapter.events():
                if isinstance(evt, TradesUpdate):
                    agg = self._aggregators.get(evt.instrument_id)
                    if agg is None:
                        continue
                    closed = agg.on_tick(evt)
                    if closed is not None:
                        await self.bus.publish(closed)
        except asyncio.CancelledError:
            return

    async def stop(self) -> None:
        if self._task is not None:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None
        await self.bus.close()
