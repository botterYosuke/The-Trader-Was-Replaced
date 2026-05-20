"""ManualOrderFacade — Phase 9 Step 2 の手動発注 dispatch（軽量 facade）。

ADR (Phase 9 §7「Step 2 は手動発注 facade」): Nautilus ExecEngine / RiskEngine の
本格 wiring は Phase 10 / LiveAuto に延期。Step 2 は `adapter.submit_order` /
`cancel_order` を直接叩き、`OrderResult` を正規化した `OrderEventData` を返す薄い
dispatch に留める。

責務:
- place(...) -> OrderEventData          : adapter.submit_order を await し正規化・track
- cancel(order_id) -> OrderEventData     : adapter.cancel_order を await し track 更新
- get_status(order_id) -> OrderEventData | None : 直近 state を参照（同期・GIL 安全）

設計メモ:
- **transport 非依存**: proto を import しない。proto 変換と `publish_backend_event`
  は gRPC handler（server_grpc.py）の責務。token / execution mode 検証 /
  VENUE_LOGIN_REQUIRED 判定も handler 側。facade は adapter 稼働前提で呼ばれる。
- place / cancel は live loop thread 上で await され、get_status は gRPC worker
  thread から同期で呼ばれる cross-thread 構造のため、`_orders` を `threading.Lock`
  で保護する（SecretVault / BackendEventBus と同じ方針。lock 内で await しない）。
- `_orders` は session lifetime で増え続ける（eviction なし）。1 セッションの手動発注
  回数程度の蓄積で漏洩面でも実害でもないため Phase 9 scope では掃除しない（SecretVault
  の `_targets`/`_ttl_armed` と同じ据え置き判断）。問題化したら TTL/max-size を後続で追加。
- 第二暗証番号 (`second_secret`) は Step 2 では受理して無視する（SecretVault 結線は
  Step 5 で Tachibana に追加。mock / kabu は不要）。adapter kwargs にも転送しない
  （平文 secret を adapter ログ・**extra に漏らさないため）。

RPC 成功セマンティクス（handler が踏襲）:
- place: 発注往復が完了すれば常に OrderEventData を返す（venue REJECTED も status に
  反映、RPC success=True）。検証エラーは OrderFacadeError を raise。
- cancel: CANCELED 成立で OrderEventData（RPC success=True）。venue が取消を拒否したら
  OrderFacadeError("CANCEL_REJECTED")（RPC success=False、元注文は store 上で不変）。
- 未知 order_id / 不正パラメータ: OrderFacadeError（RPC success=False、event 無し）。
"""
from __future__ import annotations

import threading
import time

from engine.live.adapter import InstrumentId, OrderingVenueAdapter
from engine.live.order_types import OrderEventData, OrderResult

_VALID_SIDES = {"BUY", "SELL"}
_VALID_ORDER_TYPES = {"MARKET", "LIMIT"}


class OrderFacadeError(Exception):
    """facade レベルの既知エラー。`error_code` を gRPC Res にそのまま載せる。"""

    def __init__(self, error_code: str) -> None:
        super().__init__(error_code)
        self.error_code = error_code


def _now_ms() -> int:
    return int(time.time() * 1000)


class ManualOrderFacade:
    def __init__(self, adapter: OrderingVenueAdapter) -> None:
        self._adapter = adapter
        # client_order_id -> 直近の正規化済みイベント（GetOrderStatus 用 in-memory store）
        self._orders: dict[str, OrderEventData] = {}
        # cross-thread (live-loop write / gRPC-worker read) のため dict 操作を保護。
        self._lock = threading.Lock()

    async def place(
        self,
        *,
        venue: str,
        instrument_id: InstrumentId,
        side: str,
        qty: float,
        order_type: str,
        time_in_force: str,
        price: float | None = None,
        second_secret: str | None = None,  # Step 2 では受理して無視（Step 5 で結線）
    ) -> OrderEventData:
        side_n = side.upper()
        type_n = order_type.upper()
        if side_n not in _VALID_SIDES:
            raise OrderFacadeError("INVALID_SIDE")
        if type_n not in _VALID_ORDER_TYPES:
            raise OrderFacadeError("INVALID_ORDER_TYPE")
        if qty <= 0:
            raise OrderFacadeError("INVALID_QTY")
        if type_n == "LIMIT" and (price is None or price <= 0):
            raise OrderFacadeError("INVALID_PRICE")

        # MARKET は price を venue に渡さない（指値解釈の取り違えを防ぐ）。
        effective_price = price if type_n == "LIMIT" else None

        res: OrderResult = await self._adapter.submit_order(
            venue=venue,
            instrument_id=instrument_id,
            side=side_n,
            qty=qty,
            price=effective_price,
            order_type=type_n,
            time_in_force=time_in_force,
        )

        event = OrderEventData(
            order_id=res.client_order_id,
            venue_order_id="",
            client_order_id=res.client_order_id,
            status=res.status,
            filled_qty=res.filled_qty,
            avg_price=res.avg_price if res.avg_price is not None else 0.0,
            ts_ms=_now_ms(),
        )
        with self._lock:
            self._orders[event.order_id] = event
        return event

    async def cancel(
        self,
        *,
        venue: str,
        order_id: str,
        second_secret: str | None = None,  # Step 2 では受理して無視
    ) -> OrderEventData:
        with self._lock:
            prior = self._orders.get(order_id)
        if prior is None:
            raise OrderFacadeError("UNKNOWN_ORDER_ID")

        res: OrderResult = await self._adapter.cancel_order(
            venue=venue,
            order_id=order_id,
        )

        if res.status == "REJECTED":
            # 取消拒否: 元注文は live のまま。store は変更しない。
            raise OrderFacadeError("CANCEL_REJECTED")

        # CANCELED 成立: 既存の約定量 / 平均価格は維持したまま終端状態に遷移させる
        # （取消は約定済み数量を巻き戻さない）。
        event = OrderEventData(
            order_id=order_id,
            venue_order_id=prior.venue_order_id,
            client_order_id=order_id,
            status="CANCELED",
            filled_qty=prior.filled_qty,
            avg_price=prior.avg_price,
            ts_ms=_now_ms(),
        )
        with self._lock:
            self._orders[order_id] = event
        return event

    def get_status(self, order_id: str) -> OrderEventData | None:
        """同期参照（gRPC worker thread から呼ばれる）。"""
        with self._lock:
            return self._orders.get(order_id)
