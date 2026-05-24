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
  （平文 secret を adapter ログ・**extra に漏らさないため）。**この param は facade で
  意図的に終端する**（adapter には届かない）。実際の Tachibana secret 経路は SecretVault
  （`SecretRequired` push → `SubmitSecret` RPC、§1.3）であり、`PlaceOrderReq.second_secret`
  と二重チャネルになる懸念は Step 5 で一本化する（Phase 9 plan の Step 5/6 handoff 参照）。
- **timeout 注意（Step 5/6 向け）**: gRPC handler は `future.result(timeout)` で待つ。実
  venue adapter が timeout を超えて応答した場合、RPC は失敗を返すが **注文は venue 側で
  成立している可能性がある**（mock は即時応答のため Step 2 では発生しない）。handler は
  この場合 `PLACE_TIMEOUT` / `CANCEL_TIMEOUT` を返し、UI に「結果不明・venue で要確認」を
  促す。reconciliation（GetOrders 突合）は Step 8 で実装する。

RPC 成功セマンティクス（handler が踏襲）:
- place: 発注往復が完了すれば常に OrderEventData を返す（venue REJECTED も status に
  反映、RPC success=True）。検証エラーは OrderFacadeError を raise。
- cancel: CANCELED 成立で OrderEventData（RPC success=True）。venue が取消を拒否したら
  OrderFacadeError("CANCEL_REJECTED")（RPC success=False、元注文は store 上で不変）。
  既に終端状態（FILLED 等）の注文は OrderFacadeError("ORDER_NOT_CANCELABLE")（venue 未到達）。
- 未知 order_id / 不正パラメータ: OrderFacadeError（RPC success=False、event 無し）。
"""
from __future__ import annotations

import math
import threading
import time

from engine.live.adapter import InstrumentId, OrderingVenueAdapter
from engine.live.order_types import VALID_ORDER_STATUSES, OrderEventData, OrderResult

_VALID_SIDES = {"BUY", "SELL"}
_VALID_ORDER_TYPES = {"MARKET", "LIMIT"}
# 取消できない終端状態（Nautilus OrderStatus の部分集合）。これらの注文の cancel は
# venue に送らず ORDER_NOT_CANCELABLE で弾き、終端注文を CANCELED へ上書きする矛盾
# イベント（FILLED 注文が filled_qty 全量のまま "CANCELED" 化）の publish を防ぐ
# （plan §1.2 OrderStateMachine 準拠）。
_TERMINAL_STATUSES = {"FILLED", "CANCELED", "REJECTED", "EXPIRED", "DENIED"}
# 二重定義のドリフト防止: 終端集合は必ず正規の OrderStatus 名の部分集合であること。
assert _TERMINAL_STATUSES <= VALID_ORDER_STATUSES


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
        if not venue:
            raise OrderFacadeError("INVALID_VENUE")
        if not instrument_id:
            raise OrderFacadeError("INVALID_INSTRUMENT")
        if side_n not in _VALID_SIDES:
            raise OrderFacadeError("INVALID_SIDE")
        if type_n not in _VALID_ORDER_TYPES:
            raise OrderFacadeError("INVALID_ORDER_TYPE")
        # NaN/Inf は proto double で wire 通過する。`<= 0` は NaN を弾けない
        # （NaN との比較は常に False）ため isfinite を明示。
        if not math.isfinite(qty) or qty <= 0:
            raise OrderFacadeError("INVALID_QTY")
        if type_n == "LIMIT" and (price is None or not math.isfinite(price) or price <= 0):
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
            # issue #29 Slice3a: 静的属性を載せて GetOrders seed で完全行を復元可能にする。
            # symbol は instrument_id、price は LIMIT のときだけ（MARKET は指値なし → None）。
            symbol=instrument_id,
            side=side_n,
            qty=qty,
            price=effective_price,
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
        if prior.status in _TERMINAL_STATUSES:
            raise OrderFacadeError("ORDER_NOT_CANCELABLE")

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
            # Slice3a: 取消は静的属性を変えない → 元注文の symbol/side/qty/price を保持。
            symbol=prior.symbol,
            side=prior.side,
            qty=prior.qty,
            price=prior.price,
        )
        with self._lock:
            self._orders[order_id] = event
        return event

    async def modify(
        self,
        *,
        venue: str,
        order_id: str,
        new_price: float | None = None,
        new_qty: float | None = None,
        second_secret: str | None = None,  # Step 4 では受理して無視（Step 5 で結線）
    ) -> OrderEventData:
        """既存注文の訂正（価格 / 数量）。adapter.modify_order に委譲する。

        **OrderEvent に qty/price が載らない設計の帰結**: `OrderEventData` は ids +
        status + fill（filled_qty / avg_price）のみで、注文の数量・価格・銘柄・売買区分を
        持たない。よって facade が返す event は status（adapter 応答、例 ACCEPTED）/
        filled_qty / avg_price / venue_order_id を更新するに留まり、訂正後の
        **新数量 / 新価格は載らない**。UI への qty/price 反映は Rust 側が ModifyOrder
        コマンドの new_qty / new_price から行う（OrderEvent に当該 field が無いため）。
        """
        with self._lock:
            prior = self._orders.get(order_id)
        if prior is None:
            raise OrderFacadeError("UNKNOWN_ORDER_ID")
        if prior.status in _TERMINAL_STATUSES:
            raise OrderFacadeError("ORDER_NOT_MODIFIABLE")
        if new_price is None and new_qty is None:
            raise OrderFacadeError("NOTHING_TO_MODIFY")
        # 指定された値のみ検証（None は「変更しない」を意味する）。NaN/Inf は proto
        # double で wire 通過するため isfinite を明示（place の流儀踏襲）。
        if new_price is not None and (not math.isfinite(new_price) or new_price <= 0):
            raise OrderFacadeError("INVALID_PRICE")
        if new_qty is not None and (not math.isfinite(new_qty) or new_qty <= 0):
            raise OrderFacadeError("INVALID_QTY")

        res: OrderResult = await self._adapter.modify_order(
            venue=venue,
            order_id=order_id,
            new_price=new_price,
            new_qty=new_qty,
        )

        if res.status == "REJECTED":
            # 訂正拒否: 元注文は live のまま。store は変更しない。
            raise OrderFacadeError("MODIFY_REJECTED")

        # 訂正受理: adapter 応答 status を反映。fill 量は adapter が 0/None を返す場合
        # 既存の約定量 / 平均価格を維持する（訂正は約定済み数量を巻き戻さない。
        # cancel の fill 保全と同方針）。
        event = OrderEventData(
            order_id=order_id,
            venue_order_id=prior.venue_order_id,
            client_order_id=order_id,
            status=res.status,
            filled_qty=res.filled_qty if res.filled_qty else prior.filled_qty,
            avg_price=res.avg_price if res.avg_price is not None else prior.avg_price,
            ts_ms=_now_ms(),
            # Slice3a: symbol/side は不変。qty/price は指定された方だけ更新（None は据え置き、
            # proto optional セマンティクス）。UI の qty/price 反映は従来 Rust が
            # ModifyOrder コマンドの new_qty/new_price から行うが、store もここで一貫させる。
            symbol=prior.symbol,
            side=prior.side,
            qty=new_qty if new_qty is not None else prior.qty,
            price=new_price if new_price is not None else prior.price,
        )
        with self._lock:
            self._orders[order_id] = event
        return event

    def get_status(self, order_id: str) -> OrderEventData | None:
        """同期参照（gRPC worker thread から呼ばれる）。"""
        with self._lock:
            return self._orders.get(order_id)

    def list_orders(self) -> list[OrderEventData]:
        """稼働中（非終端）注文の snapshot（GetOrders / §3.8 reconcile 用、同期参照）。

        終端注文（FILLED/CANCELED/...）は「稼働中」ではないので除外する。再起動直後の
        fresh backend はこの store が空なので [] を返す（= UI 楽観的状態との diff で
        「状態不明」を炙り出す reconcile primitive）。
        """
        with self._lock:
            return [e for e in self._orders.values() if e.status not in _TERMINAL_STATUSES]
