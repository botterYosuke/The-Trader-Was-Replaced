"""engine.live.nautilus_exec_client — 既存 venue adapter を Nautilus に bridge する
LiveExecutionClient (Phase 10 §1.1 / §2.4 / Step 4)。

Phase 9 の `OrderingVenueAdapter`（`submit_order` / `cancel_order` / `modify_order` /
`fetch_account`、bespoke 約定 facade）を Nautilus の `LiveExecutionClient` 契約に被せる。
これにより Strategy → `Trader` → `RiskEngine`（ネイティブ rail）→ `ExecutionEngine` →
本 client → adapter → venue の正規パイプラインが通り、Phase 10 LiveAuto が初めて
「戦略コードからの自動発注」を行える。

Safety Rails の二段構え（§2.4）:
- **ネイティブ pre-trade**（`max_order_value` / `max_orders_per_minute`）は `RiskEngine` が
  本 client に到達する前に評価する（`LiveRiskEngineConfig`、controller が構成）。
- **独自 pre-trade**（`max_position_size_jpy` / `allowed_instruments`）は本 client の
  `_submit_order` 内で `SafetyRails.check_pre_trade()` を評価し、違反なら venue に送らず
  `generate_order_denied()` + `on_safety_violation` callback で `SafetyRailViolation` を push。

PAUSE ゲート（Issue #6 / §1.2）:
- 当該 run が PAUSED の間（`state_machine.is_running` が False）は host が新規発注ゲートを
  閉じる。本 client は `_submit_order` の **SUBMITTED 前** に `is_run_gated(strategy_id)` を
  確認し、ゲートが閉じていれば venue に送らず `generate_order_denied()` で deny する。
  ゲート判定は controller 経由で注入される（StrategyId → run state machine を参照）。

注意（Step 4 の射程）:
- market data／bar 供給は Step 8。よって MARKET 注文の参照価格が cache に無い場合、
  notional は LIMIT 値か 0 にフォールバックし position-size チェックを保守的にスキップする
  （§8 Open Risk: market data が来るまで notional 評価は限定的）。
- fill は adapter の `OrderResult`（status/filled_qty/avg_price）を 1 イベントに正規化する。
  partial の段階的 fill ストリームは venue adapter 側の責務（Step 5/6 で EC により駆動）。
"""

from __future__ import annotations

import logging
from typing import Any, Callable, Optional

from nautilus_trader.execution.messages import CancelOrder, ModifyOrder, SubmitOrder
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.enums import (
    AccountType,
    LiquiditySide,
    OmsType,
)
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientId,
    TradeId,
    Venue,
    VenueOrderId,
)
from nautilus_trader.model.objects import AccountBalance, Money

from engine.live.order_types import OrderResult
from engine.live.safety_rails import RailViolation, SafetyRails

log = logging.getLogger(__name__)

# adapter が返す Nautilus OrderStatus 名のうち「約定あり」を表す集合。
_FILLED_STATUSES = {"FILLED", "PARTIALLY_FILLED"}
# 終端 status 集合（order_facade._TERMINAL_STATUSES のローカル写し。cross-module 結合を
# 増やさないため複製。両者は正規 OrderStatus 名の部分集合という不変条件で揃える）。
_TERMINAL_STATUSES = {"FILLED", "CANCELED", "REJECTED", "EXPIRED", "DENIED"}


class NautilusVenueExecClient(LiveExecutionClient):
    """`OrderingVenueAdapter` を包む `LiveExecutionClient`。

    各メソッドは Nautilus が live loop 上で await する。発注主体（StrategyId）は
    `command.strategy_id` がそのまま運ぶので、本 client は venue 横断で単一でよい。
    """

    def __init__(
        self,
        *,
        loop,
        venue: Venue,
        msgbus,
        cache,
        clock,
        adapter: Any,  # OrderingVenueAdapter（循環 import 回避で Any）
        safety_rails: SafetyRails,
        instrument_provider,
        on_safety_violation: Optional[Callable[[RailViolation], None]] = None,
        is_run_gated: Optional[Callable[[str], bool]] = None,
        config=None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(venue.value),
            venue=venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            base_currency=JPY,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )
        self._adapter = adapter
        self._rails = safety_rails
        self._on_safety_violation = on_safety_violation
        # Issue #6: 当該注文を出した run（StrategyId）が PAUSED で新規発注ゲートが
        # 閉じているかを返す seam。`(strategy_id_str) -> bool`、True なら deny する。
        # None の場合は gate 概念が無い文脈（手動発注経路 / 単体テスト）として素通し。
        self._is_run_gated = is_run_gated
        self._venue_str = venue.value

    # ── connection ───────────────────────────────────────────────────────────

    async def _connect(self) -> None:
        # adapter の login は live session（_backend_impl）が既に済ませている（共有所有権、
        # §1.1）。ここでは追加の接続は張らず、口座スナップショットだけ seed する
        # （RiskEngine の free-balance チェックと Portfolio が account を要求するため）。
        await self._seed_account_state()

    async def _seed_account_state(self) -> None:
        # generate_account_state は account_id を要求するので接続時に採番する。
        if self.account_id is None:
            self._set_account_id(AccountId(f"{self._venue_str}-001"))
        try:
            snapshot = await self._adapter.fetch_account()
        except Exception:  # noqa: BLE001 — account 取得失敗は seed をスキップ（後続 sync に委ねる）
            log.warning("fetch_account failed during connect; account state not seeded")
            return
        cash = float(getattr(snapshot, "cash", 0.0) or 0.0)
        balance = AccountBalance(
            total=Money(cash, JPY),
            locked=Money(0, JPY),
            free=Money(cash, JPY),
        )
        self.generate_account_state(
            balances=[balance],
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _disconnect(self) -> None:
        pass

    # ── order commands ─────────────────────────────────────────────────────────

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        # Issue #6: PAUSE ゲート。当該 run（StrategyId）が PAUSED なら新規発注を venue に
        # 送らず deny する（state_machine.is_running が False → host が発注ゲートを閉じる）。
        # rails と同じく **SUBMITTED 前**（INITIALIZED）に判定する。OrderDenied は
        # INITIALIZED からのみ有効な遷移のため、generate_order_submitted より前に deny しないと
        # 「SUBMITTED → DENIED」が不正遷移として落ち、注文が SUBMITTED で固着する。
        if self._is_run_gated is not None and self._is_run_gated(order.strategy_id.value):
            self.generate_order_denied(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                "STRATEGY_PAUSED: new orders are gated while the run is PAUSED",
                self._clock.timestamp_ns(),
            )
            return

        # 独自 pre-trade rails を **SUBMITTED 前** に評価する（ネイティブ rail は RiskEngine が
        # 既に通過させている）。OrderDenied は INITIALIZED からのみ有効な遷移なので、
        # generate_order_submitted より前に deny しないと「SUBMITTED → DENIED」が
        # 不正遷移として落ち、注文が SUBMITTED で固着する。
        violation = self._rails.check_pre_trade(
            instrument_id=order.instrument_id.value,
            order_notional_jpy=self._order_notional(order),
            current_position_value_jpy=self._position_value(order.instrument_id),
        )
        if violation is not None:
            self.generate_order_denied(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                f"{violation.kind}: {violation.detail}",
                self._clock.timestamp_ns(),
            )
            if self._on_safety_violation is not None:
                self._on_safety_violation(violation)
            return

        # rails 通過 → SUBMITTED を確定（venue 往復前に楽観的に遷移）。
        self.generate_order_submitted(
            order.strategy_id, order.instrument_id, order.client_order_id, self._clock.timestamp_ns()
        )

        price = float(order.price) if order.has_price else None
        try:
            res: OrderResult = await self._adapter.submit_order(
                venue=self._venue_str,
                instrument_id=order.instrument_id.value,
                side=order.side.name,
                qty=float(order.quantity),
                price=price,
                order_type=order.order_type.name,
                time_in_force=order.time_in_force.name,
                client_order_id=order.client_order_id.value,
            )
        except Exception as exc:  # noqa: BLE001 — venue/adapter 失敗は REJECTED に正規化
            log.exception("submit_order adapter call failed")
            self.generate_order_rejected(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                f"ADAPTER_ERROR: {exc}",
                self._clock.timestamp_ns(),
            )
            return

        self._apply_submit_result(order, res)

    async def _cancel_order(self, command: CancelOrder) -> None:
        try:
            res: OrderResult = await self._adapter.cancel_order(
                venue=self._venue_str, order_id=command.client_order_id.value
            )
        except Exception as exc:  # noqa: BLE001
            log.exception("cancel_order adapter call failed")
            self.generate_order_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                f"CANCEL_ADAPTER_ERROR: {exc}",
                self._clock.timestamp_ns(),
            )
            return
        ts = self._clock.timestamp_ns()
        if res.status == "REJECTED":
            # 取消拒否: 元注文は live のまま。CANCELED へは遷移させない。
            return
        self.generate_order_canceled(
            command.strategy_id,
            command.instrument_id,
            command.client_order_id,
            command.venue_order_id or self._synth_venue_order_id(command.client_order_id),
            ts,
        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        new_price = float(command.price) if command.price is not None else None
        new_qty = float(command.quantity) if command.quantity is not None else None
        try:
            res: OrderResult = await self._adapter.modify_order(
                venue=self._venue_str,
                order_id=command.client_order_id.value,
                new_price=new_price,
                new_qty=new_qty,
            )
        except Exception as exc:  # noqa: BLE001
            log.exception("modify_order adapter call failed")
            return
        ts = self._clock.timestamp_ns()
        venue_order_id = command.venue_order_id or self._synth_venue_order_id(
            command.client_order_id
        )
        # Issue #12: modify が終端 status を返したら対応する終端イベントを emit する。
        # generate_order_updated のままだと CANCELED/EXPIRED でも ACCEPTED 据え置きになり、
        # 「約定済みのはずが Nautilus 上 live」という危険な乖離になる。
        if res.status == "CANCELED":
            self.generate_order_canceled(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                ts,
            )
            return
        if res.status == "EXPIRED":
            self.generate_order_expired(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                ts,
            )
            return
        if res.status in _FILLED_STATUSES and res.filled_qty > 0:
            order = self._cache.order(command.client_order_id)
            instrument = self._cache.instrument(command.instrument_id)
            if order is None or instrument is None:
                # 防御: cache に無ければ filled を合成できない。live 据え置き。
                log.warning("modify→FILLED but order/instrument missing in cache")
                return
            last_px = res.avg_price if res.avg_price else (
                float(order.price) if order.has_price else 0.0
            )
            self.generate_order_filled(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                None,  # venue_position_id → NETTING なので engine が解決
                TradeId(f"{command.client_order_id.value}-1"),
                order.side,
                order.order_type,
                instrument.make_qty(res.filled_qty),
                instrument.make_price(last_px),
                instrument.quote_currency,
                Money(0, instrument.quote_currency),  # commission（venue 手数料は Step 5/6）
                LiquiditySide.TAKER,
                self._clock.timestamp_ns(),
            )
            return
        # Issue #12 案A: 拒否系 terminal（REJECTED / DENIED 等）は更新を適用せず、order を
        # modify 前状態(ACCEPTED)へ戻す。generate_order_modify_rejected は FSM 上 order が
        # PENDING_UPDATE のとき元状態へ復帰させる（終端化しない）。これで PENDING_UPDATE
        # 固着も「終端を新価格適用 ACCEPTED で live 据え置き」も解消する。
        # 注: ここに到達する terminal は上で return 済みの CANCELED/EXPIRED/約定 FILLED を
        # 除いた REJECTED / DENIED と、約定情報の無い filled_qty==0 の FILLED 系。後者を
        # modify_rejected 扱いにするのは「約定の無い終端 modify は更新拒否」で安全側に妥当。
        if res.status in _TERMINAL_STATUSES:
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                res.reject_reason or res.status,
                ts,
            )
            return
        # Issue #12 付随バグ: price-only modify など command.quantity / command.price が
        # None のとき、Quantity/Price 型を要求する generate_order_updated に None を渡すと
        # TypeError で更新イベントが emit されず PENDING_UPDATE に固着する。
        # cache の現値で据え置き埋めする（FILLED 分岐と同流儀の order None 防御を踏襲）。
        order = self._cache.order(command.client_order_id)
        if order is None:
            log.warning("modify→UPDATED but order missing in cache; cannot fill defaults")
            return
        upd_qty = command.quantity if command.quantity is not None else order.quantity
        upd_price = command.price if command.price is not None else (
            order.price if order.has_price else None
        )
        self.generate_order_updated(
            command.strategy_id,
            command.instrument_id,
            command.client_order_id,
            venue_order_id,
            upd_qty,
            upd_price,
            None,  # trigger_price
            ts,
        )

    # ── helpers ─────────────────────────────────────────────────────────────────

    def _apply_submit_result(self, order, res: OrderResult) -> None:
        ts = self._clock.timestamp_ns()
        if res.status == "REJECTED":
            self.generate_order_rejected(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                res.reject_reason or "REJECTED",
                ts,
            )
            return

        venue_order_id = self._synth_venue_order_id(order.client_order_id)
        self.generate_order_accepted(
            order.strategy_id, order.instrument_id, order.client_order_id, venue_order_id, ts
        )

        if res.status in _FILLED_STATUSES and res.filled_qty > 0:
            instrument = self._cache.instrument(order.instrument_id)
            last_px = res.avg_price if res.avg_price else (
                float(order.price) if order.has_price else 0.0
            )
            self.generate_order_filled(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                venue_order_id,
                None,  # venue_position_id → NETTING なので engine が解決
                TradeId(f"{order.client_order_id.value}-1"),
                order.side,
                order.order_type,
                instrument.make_qty(res.filled_qty),
                instrument.make_price(last_px),
                instrument.quote_currency,
                Money(0, instrument.quote_currency),  # commission（venue 手数料は Step 5/6）
                LiquiditySide.TAKER,
                self._clock.timestamp_ns(),
            )

    def _synth_venue_order_id(self, client_order_id) -> VenueOrderId:
        # mock/現状 adapter は venue 採番 id を OrderResult に載せないため client_order_id
        # から合成する（Step 5/6 で実 venue_order_id に差し替え）。
        return VenueOrderId(client_order_id.value)

    def _order_notional(self, order) -> float:
        """新規注文の概算約定金額（JPY）。price 不明（MARKET で market data 未供給）なら 0。"""
        if order.has_price:
            return float(order.price) * float(order.quantity)
        instrument = self._cache.instrument(order.instrument_id)
        last = self._cache.price(order.instrument_id, _LAST) if instrument else None
        if last is not None:
            return float(last) * float(order.quantity)
        return 0.0

    def _position_value(self, instrument_id) -> float:
        """当該 instrument の既存ポジション評価額（JPY、|qty|×avg_px）。無ければ 0。"""
        total = 0.0
        for pos in self._cache.positions_open(instrument_id=instrument_id):
            total += abs(float(pos.quantity)) * float(pos.avg_px_open)
        return total

    # report 系（reconciliation 用）。Step 4 では未使用なので空 report を返す。
    async def generate_order_status_reports(self, command):  # pragma: no cover
        return []

    async def generate_order_status_report(self, command):  # pragma: no cover
        return None

    async def generate_fill_reports(self, command):  # pragma: no cover
        return []

    async def generate_position_status_reports(self, command):  # pragma: no cover
        return []


# PriceType.LAST を遅延 import（モジュール import コストと循環回避）。
from nautilus_trader.model.enums import PriceType as _PriceType  # noqa: E402

_LAST = _PriceType.LAST
