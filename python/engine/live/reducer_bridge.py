"""LiveReducerBridge — live KlineUpdate → replay reducer event 変換 + apply (Phase 8 Step 2e).

責務:
- 純関数で live `KlineUpdate` (pydantic, ts_ns) を replay `KlineUpdate` (dataclass,
  timestamp_ms) と `ReplayTimeUpdated` に変換する。
- `LiveEventBus` を購読し、KlineUpdate が来たら `data_engine.apply_replay_event`
  に `ReplayTimeUpdated -> KlineUpdate` の順で流す（§4.3 順序不変条件）。
- DepthUpdate / TradesUpdate は reducer の関心外なので無視する。

設計判断:
- volume は reducer の KlineUpdate に格納欄が無いため捨てる（Phase 8 では UI/戦略
  ともに OHLC のみ参照）。volume を残す必要が出たら reducer 側を拡張する。
- bridge は data_engine の Protocol を持たず duck typing。テスト用 stub を許容。
- 起動順序: bus.subscribe() を bridge.start() の中で同期的に行ってから task spawn。
  外部 publish より先に subscribe が必ず完了している（§7 ADR）。
"""
from __future__ import annotations

import asyncio
from typing import AsyncIterator, Optional, Protocol

from engine.live.adapter import KlineUpdate as LiveKlineUpdate
from engine.live.event_bus import LiveEventBus
from engine.reducer import (
    KlineUpdate as ReducerKlineUpdate,
    ReplayEvent,
    ReplayTimeUpdated,
)


def _ns_to_ms(ts_ns: int) -> int:
    return ts_ns // 1_000_000


def live_kline_to_reducer_kline(live: LiveKlineUpdate) -> ReducerKlineUpdate:
    ts_ms = _ns_to_ms(live.ts_ns)
    return ReducerKlineUpdate(
        timestamp_ms=ts_ms,
        open_time_ms=ts_ms,
        open=live.open,
        high=live.high,
        low=live.low,
        close=live.close,
    )


def live_kline_to_replay_time_updated(live: LiveKlineUpdate) -> ReplayTimeUpdated:
    return ReplayTimeUpdated(timestamp_ms=_ns_to_ms(live.ts_ns))


class _DataEngineLike(Protocol):
    def apply_replay_event(self, event: ReplayEvent) -> None: ...


class LiveReducerBridge:
    """bus → reducer/DataEngine の薄い橋。

    - `start()` で bus.subscribe() を取得し、消費 task を spawn する。
    - `stop()` で task を cancel→await。bus 側が先に close された場合も
      iterator が綺麗に終端するため、追加処理は不要。
    """

    def __init__(self, bus: LiveEventBus, data_engine: _DataEngineLike) -> None:
        self._bus = bus
        self._data_engine = data_engine
        self._task: Optional[asyncio.Task[None]] = None
        self._iter: Optional[AsyncIterator] = None

    async def start(self) -> None:
        if self._task is not None:
            return
        # subscribe は同期完了させてから task spawn (§7 起動順序 ADR)
        self._iter = self._bus.subscribe()
        self._task = asyncio.create_task(self._run())

    async def _run(self) -> None:
        assert self._iter is not None
        try:
            async for evt in self._iter:
                if isinstance(evt, LiveKlineUpdate):
                    self._data_engine.apply_replay_event(
                        live_kline_to_replay_time_updated(evt)
                    )
                    self._data_engine.apply_replay_event(
                        live_kline_to_reducer_kline(evt)
                    )
                # DepthUpdate / TradesUpdate は無視
        except asyncio.CancelledError:
            return

    async def stop(self) -> None:
        if self._task is None:
            return
        self._task.cancel()
        try:
            await self._task
        except asyncio.CancelledError:
            pass
        self._task = None
