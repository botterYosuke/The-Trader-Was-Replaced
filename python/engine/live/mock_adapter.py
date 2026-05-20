"""MockVenueAdapter — deterministic mock for live_runner tests (Phase 8 Step C-1).

LiveVenueAdapter Protocol の最小 no-op 実装。venue_id は "MOCK"。
fetch_instruments / subscribe / events 等の振る舞いは後続 C-2 以降で
inject API と共に拡張する。
"""

from __future__ import annotations

import asyncio
import uuid
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
from engine.live.order_types import OrderResult


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
        self._next_order_outcome: dict | None = None

    async def login(self, creds: VenueCredentials) -> None:
        self.is_logged_in = True

    async def logout(self) -> None:
        # logout は session 終了相当: 購読も内部 queue もクリアする (C-8)。
        # 実 venue の WebSocket 切断時と同じ意味論。
        self.is_logged_in = False
        self._subscribed.clear()
        while not self._queue.empty():
            self._queue.get_nowait()

    def _require_login(self) -> None:
        if not self.is_logged_in:
            raise RuntimeError("MockVenueAdapter is not logged in")

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        self._require_login()
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
        self._require_login()
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

    def set_next_order_outcome(
        self,
        *,
        status: str,
        filled_qty: float | None = None,
        avg_price: float | None = None,
        reject_reason: str | None = None,
    ) -> None:
        """テスト専用: 次の submit_order の結果を仕込む（one-shot, inject_tick 流）。

        仕込み無しなら submit_order は既定 FILLED 全約定。status="REJECTED" 時は
        filled_qty を 0 に強制する。consume 後は None に戻り、以降は既定に戻る。
        """
        self._next_order_outcome = {
            "status": status,
            "filled_qty": filled_qty,
            "avg_price": avg_price,
            "reject_reason": reject_reason,
        }

    async def submit_order(
        self,
        *,
        venue: str,
        instrument_id: InstrumentId,
        side: str,
        qty: float,
        price: float | None = None,
        order_type: str,
        time_in_force: str,
        **extra: object,
    ) -> OrderResult:
        """MockVenueAdapter 固有の注文発注（Protocol 外）。

        set_next_order_outcome で仕込みがあれば one-shot 消費し、無ければ
        既定 FILLED 全約定（filled_qty=qty, avg_price=price）。client_order_id
        は毎回 uuid 生成。secret/機密は扱わない（mock）。
        """
        self._require_login()
        client_order_id = uuid.uuid4().hex

        outcome = self._next_order_outcome
        self._next_order_outcome = None

        if outcome is None:
            return OrderResult(
                status="FILLED",
                filled_qty=qty,
                avg_price=price,
                client_order_id=client_order_id,
                reject_reason=None,
            )

        status = outcome["status"]
        if status == "REJECTED":
            return OrderResult(
                status="REJECTED",
                filled_qty=0.0,
                avg_price=None,
                client_order_id=client_order_id,
                reject_reason=outcome["reject_reason"],
            )

        # PARTIALLY_FILLED / その他: 注入 filled_qty があれば採用、無ければ qty。
        filled_qty = outcome["filled_qty"]
        if filled_qty is None:
            filled_qty = qty
        avg_price = outcome["avg_price"] if outcome["avg_price"] is not None else price
        return OrderResult(
            status=status,
            filled_qty=filled_qty,
            avg_price=avg_price,
            client_order_id=client_order_id,
            reject_reason=outcome["reject_reason"],
        )
