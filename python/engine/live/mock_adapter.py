"""MockVenueAdapter — deterministic mock for live_runner tests (Phase 8 Step C-1).

LiveVenueAdapter Protocol の最小 no-op 実装。venue_id は "MOCK"。
fetch_instruments / subscribe / events 等の振る舞いは後続 C-2 以降で
inject API と共に拡張する。
"""

from __future__ import annotations

import asyncio
from typing import AsyncIterator

from engine.live.adapter import (
    Channel,
    DepthLevel,
    DepthUpdate,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    VenueCredentials,
)


class MockVenueAdapter:
    """LiveVenueAdapter Protocol を満たす最小 mock。

    C-1 では Protocol 適合のみを担保する。実際の event 注入や
    instrument 応答は C-2 以降で追加する。
    """

    venue_id: str = "MOCK"

    def __init__(self) -> None:
        self.is_logged_in: bool = False
        self._subscribed: dict[InstrumentId, set[Channel]] = {}
        self._queue: asyncio.Queue[LiveEvent] = asyncio.Queue()

    async def login(self, creds: VenueCredentials) -> None:
        self.is_logged_in = True

    async def logout(self) -> None:
        self.is_logged_in = False

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        return [
            InstrumentRaw(
                code="7203",
                name="トヨタ自動車",
                market="TSE",
                tick_size=0.5,
                lot_size=100,
            ),
            InstrumentRaw(
                code="9984",
                name="ソフトバンクグループ",
                market="TSE",
                tick_size=1.0,
                lot_size=100,
            ),
        ]

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        self._subscribed.setdefault(instrument_id, set()).update(channels)

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        self._subscribed.pop(instrument_id, None)

    def inject_tick(self, event: LiveEvent) -> None:
        """テスト専用: subscribe 済み instrument の event を内部 queue に積む。

        C-4a では subscribe 済みのみ受け付ける最小フィルタを入れる。
        未 subscribe の厳密 reject 動作は C-4b で別テストと共に追加する。
        """
        if event.instrument_id in self._subscribed:
            self._queue.put_nowait(event)

    def emit_depth_snapshot(
        self,
        instrument_id: InstrumentId,
        ts_ns: int,
        bids: list[DepthLevel],
        asks: list[DepthLevel],
    ) -> None:
        """テスト専用: subscribe 済み instrument の DepthUpdate を内部 queue に積む。

        inject_tick と同様、subscribe gating（unsubscribe 後は no-op）を共有する。
        bids/asks は呼び出し側で DepthLevel に整形済みのものを渡す。
        """
        event = DepthUpdate(
            kind="depth",
            instrument_id=instrument_id,
            ts_ns=ts_ns,
            bids=bids,
            asks=asks,
        )
        self.inject_tick(event)

    async def events(self) -> AsyncIterator[LiveEvent]:
        while True:
            evt = await self._queue.get()
            yield evt
