"""LastPriceCache — sidebar 最新価格列向けの quote 優先 / trade fallback キャッシュ.

責務 (Phase 8 follow-up):
- DepthUpdate の best bid/ask が両方揃ったとき mid=(bid+ask)/2 を quote_mid に保持。
  片側欠の DepthUpdate は前回値を上書きしない (前値保持)。
- TradesUpdate.price は last_trade に保持し、quote_mid が無いときの fallback。
- KlineUpdate は無視。
- snapshot() は instrument の和集合に対し quote_mid 優先 / last_trade fallback の
  新規 dict を返す (外部からの mutate で内部状態が壊れない)。

設計判断:
- bus.subscribe() は start() 内で同期完了させてから task spawn (§7 起動順序 ADR)。
- _run の例外は CancelledError 以外を _last_error に格納して silent dead させない
  (reducer_bridge §9.8 ADR と同形)。
- LiveReducerBridge とは独立 (bus を共有するだけ)。
"""
from __future__ import annotations

import asyncio
from typing import AsyncIterator, Optional

from engine.live.adapter import DepthUpdate, KlineUpdate, TradesUpdate
from engine.live.event_bus import LiveEventBus


class LastPriceCache:
    def __init__(self, bus: LiveEventBus) -> None:
        self._bus = bus
        self._quote_mid: dict[str, float] = {}
        self._last_trade: dict[str, float] = {}
        self._last_kline: dict[str, float] = {}  # D27: KlineUpdate.close fallback
        self._task: Optional[asyncio.Task[None]] = None
        self._iter: Optional[AsyncIterator] = None
        self._last_error: Optional[BaseException] = None

    async def start(self) -> None:
        if self._task is not None and not self._task.done():
            return
        self._last_error = None
        self._iter = self._bus.subscribe()
        self._task = asyncio.create_task(self._run())

    async def _run(self) -> None:
        assert self._iter is not None
        try:
            async for evt in self._iter:
                if isinstance(evt, DepthUpdate):
                    if evt.bids and evt.asks:
                        mid = (evt.bids[0].price + evt.asks[0].price) / 2.0
                        self._quote_mid[evt.instrument_id] = mid
                    # 片側欠は前値保持 (何もしない)
                elif isinstance(evt, TradesUpdate):
                    self._last_trade[evt.instrument_id] = evt.price
                elif isinstance(evt, KlineUpdate):
                    # D27: ingest KlineUpdate.close as fallback price
                    self._last_kline[evt.instrument_id] = evt.close
        except asyncio.CancelledError:
            return
        except BaseException as exc:
            self._last_error = exc
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

    def snapshot(self) -> dict[str, float]:
        """Return current prices: quote_mid > last_trade > last_kline priority."""
        ids = set(self._quote_mid) | set(self._last_trade) | set(self._last_kline)
        out: dict[str, float] = {}
        for iid in ids:
            if iid in self._quote_mid:
                out[iid] = self._quote_mid[iid]
            elif iid in self._last_trade:
                out[iid] = self._last_trade[iid]
            else:
                out[iid] = self._last_kline[iid]
        return out

    def remove(self, instrument_id: str) -> None:
        """D20/D27: Remove instrument from all caches to prevent stale prices on re-add."""
        self._quote_mid.pop(instrument_id, None)
        self._last_trade.pop(instrument_id, None)
        self._last_kline.pop(instrument_id, None)

    @property
    def last_error(self) -> Optional[BaseException]:
        return self._last_error
