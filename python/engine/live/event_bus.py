"""LiveEventBus — asyncio Queue ベースの fan-out（Phase 8 §3.3）。

責務:
- publish(event): 現在 subscribe 中の全 Queue に event を配る
- subscribe(): 新しい AsyncIterator を返す（以降の publish を受信、過去は replay しない）
- close(): 全 subscriber に終端 sentinel を流し、以降の publish を RuntimeError で拒否

topic / filtering は持たない（live_runner 側の責務）。
"""
from __future__ import annotations

import asyncio
from typing import AsyncIterator

from engine.live.adapter import LiveEvent


class LiveEventBus:
    def __init__(self) -> None:
        self._subscribers: list[asyncio.Queue[LiveEvent | None]] = []
        self._closed = False

    def subscribe(self) -> AsyncIterator[LiveEvent]:
        if self._closed:
            # close 済みなら即終端する iterator を返す
            q: asyncio.Queue[LiveEvent | None] = asyncio.Queue()
            q.put_nowait(None)
        else:
            q = asyncio.Queue()
            self._subscribers.append(q)
        return self._iter(q)

    async def _iter(self, q: asyncio.Queue[LiveEvent | None]) -> AsyncIterator[LiveEvent]:
        try:
            while True:
                item = await q.get()
                if item is None:
                    return
                yield item
        finally:
            if q in self._subscribers:
                self._subscribers.remove(q)

    async def publish(self, event: LiveEvent) -> None:
        if self._closed:
            raise RuntimeError("LiveEventBus is closed")
        for q in self._subscribers:
            await q.put(event)

    async def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        for q in self._subscribers:
            await q.put(None)
        self._subscribers.clear()
