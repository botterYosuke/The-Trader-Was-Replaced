"""LiveRunner — adapter → aggregator → event_bus pipeline (Phase 8 §3, Step 1+2).

責務:
- subscribe(instrument_id): adapter に {"trades", "depth"} を購読し、
  intervals_ns で指定された各 interval に対して TickBarAggregator を生成する。
- start(): adapter.events() を消費する background task を起動。
  - TradesUpdate → 当該 instrument の全 aggregator に on_tick、確定 bar を bus.publish
  - DepthUpdate / KlineUpdate (venue 直送) → そのまま bus.publish (aggregator 迂回)
- stop(): background task を cancel して await し、bus を close。

Step 2 で追加:
- DepthUpdate pass-through
- venue 直送 KlineUpdate pass-through (集約済み bar を venue が送ってくる経路)
- 複数 instrument (subscribe を複数回呼べる)
- 複数 interval (LiveRunner(intervals_ns=[60s, 300s, ...]))

Step スコープ外:
- reducer / DataEngine 接続（別途 Step 2e で converter を作る）
"""
from __future__ import annotations

import asyncio
from typing import Iterable, Optional

from engine.live.adapter import (
    DepthUpdate,
    InstrumentId,
    KlineUpdate,
    LiveVenueAdapter,
    TradesUpdate,
)
from engine.live.aggregator import TickBarAggregator
from engine.live.event_bus import LiveEventBus


class LiveRunner:
    def __init__(
        self,
        adapter: LiveVenueAdapter,
        interval_ns: Optional[int] = None,
        intervals_ns: Optional[Iterable[int]] = None,
    ) -> None:
        intervals = _normalize_intervals(interval_ns, intervals_ns)
        self._adapter = adapter
        self._intervals_ns: tuple[int, ...] = intervals
        # 各 instrument は interval ごとに 1 個の aggregator を持つ
        self._aggregators: dict[InstrumentId, list[TickBarAggregator]] = {}
        self.bus: LiveEventBus = LiveEventBus()
        self._task: Optional[asyncio.Task[None]] = None
        self._last_error: Optional[BaseException] = None

    async def subscribe(self, instrument_id: InstrumentId) -> None:
        # idempotent: 既に登録済みなら何もしない
        if instrument_id in self._aggregators:
            return
        # 先に aggregator を登録してから adapter に subscribe する。
        # こうすることで start() 後に subscribe() が呼ばれた場合でも
        # adapter.subscribe() 完了前に到着した最初の tick を取りこぼさない。
        self._aggregators[instrument_id] = [
            TickBarAggregator(instrument_id=instrument_id, interval_ns=iv)
            for iv in self._intervals_ns
        ]
        try:
            await self._adapter.subscribe(instrument_id, {"trades", "depth"})
        except BaseException:
            # adapter 側で失敗したら登録を巻き戻す
            self._aggregators.pop(instrument_id, None)
            raise

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        # idempotent: 未登録なら何もしない
        if instrument_id not in self._aggregators:
            return
        # 先に adapter に通知してから内部 state を落とす。
        # adapter.unsubscribe が失敗したら _aggregators は残す
        # (再試行可能 / 状態の真実は adapter 側)。
        await self._adapter.unsubscribe(instrument_id)
        self._aggregators.pop(instrument_id, None)

    async def start(self) -> None:
        if self._task is not None and not self._task.done():
            return
        self._last_error = None
        self._task = asyncio.create_task(self._run())

    def _is_subscribed(self, instrument_id: InstrumentId) -> bool:
        return instrument_id in self._aggregators

    async def _run(self) -> None:
        try:
            async for evt in self._adapter.events():
                # 未購読 instrument の event は一切流さない（実 adapter が global stream
                # の別銘柄 frame や unsubscribe 直後の残留 frame を出してきた場合の防衛線、§9.9 ADR）
                if not self._is_subscribed(evt.instrument_id):
                    continue
                if isinstance(evt, TradesUpdate):
                    for agg in self._aggregators[evt.instrument_id]:
                        closed = agg.on_tick(evt)
                        if closed is not None:
                            await self.bus.publish(closed)
                elif isinstance(evt, (DepthUpdate, KlineUpdate)):
                    await self.bus.publish(evt)
        except asyncio.CancelledError:
            return
        except BaseException as exc:
            self._last_error = exc
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

    @property
    def last_error(self) -> Optional[BaseException]:
        return self._last_error


def _normalize_intervals(
    interval_ns: Optional[int],
    intervals_ns: Optional[Iterable[int]],
) -> tuple[int, ...]:
    if interval_ns is None and intervals_ns is None:
        raise ValueError("either interval_ns or intervals_ns must be provided")
    if interval_ns is not None and intervals_ns is not None:
        raise ValueError("specify only one of interval_ns or intervals_ns")
    if intervals_ns is not None:
        result = tuple(int(iv) for iv in intervals_ns)
        if not result:
            raise ValueError("intervals_ns must not be empty")
    else:
        result = (int(interval_ns),)  # type: ignore[arg-type]
    for iv in result:
        if iv <= 0:
            raise ValueError("interval_ns must be positive")
    return result
