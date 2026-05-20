from __future__ import annotations

import asyncio
from typing import AsyncIterator, Optional

from engine.live.adapter import DepthUpdate
from engine.live.event_bus import LiveEventBus
from engine.models import DepthSnapshot, DepthLevel


class DepthCache:
    """最新の板 (DepthUpdate) を instrument ごとに DepthSnapshot として保持する
    bus-fed キャッシュ。LastPriceCache と同じ start/stop/_run ライフサイクル。
    Live のみで動き、GetState が snapshot() を per_instrument[id].depth に注入する。"""

    def __init__(self, bus: LiveEventBus) -> None:
        self._bus = bus
        self._depth: dict[str, DepthSnapshot] = {}
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
                    try:
                        snapshot = DepthSnapshot(
                            bids=[DepthLevel(price=l.price, size=l.size) for l in evt.bids],
                            asks=[DepthLevel(price=l.price, size=l.size) for l in evt.asks],
                            timestamp_ms=evt.ts_ns // 1_000_000,
                        )
                    except Exception:
                        # 不正な level (price<=0 / NaN 等で strict DepthLevel が reject)
                        # は該当 update だけ skip。1 銘柄の不正 tick で全 depth feed が
                        # 永久停止するのを防ぐ（consume loop は止めない）。
                        continue
                    self._depth[evt.instrument_id] = snapshot
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

    def snapshot(self) -> dict[str, DepthSnapshot]:
        return dict(self._depth)

    def remove(self, instrument_id: str) -> None:
        self._depth.pop(instrument_id, None)

    @property
    def last_error(self) -> Optional[BaseException]:
        return self._last_error
