"""engine.live.safety_rails — Live 自動戦略の Safety Rails (Phase 10 §2.4 / Step 4)。

計画書 §0.6 / §2.4 の通り、Safety Rails は **ネイティブで賄える項目** と **独自ロジックが
要る項目** に分ける:

| Rail | 実装手段 |
| --- | --- |
| `max_order_value_jpy`    | ネイティブ `LiveRiskEngineConfig.max_notional_per_order`（pre-trade） |
| `max_orders_per_minute`  | ネイティブ `LiveRiskEngineConfig.max_order_submit_rate`（pre-trade rate） |
| `max_position_size_jpy`  | 独自 pre-trade（既存ポジション金額 + 新規注文金額 ≤ 上限） |
| `allowed_instruments`    | 独自 pre-trade（ホワイトリスト照合） |
| `max_daily_loss_jpy`     | 独自 post-trade（当日 P&L が上限割れで run を ERROR に） |

この module は **transport / engine 非依存の純粋ロジック**:
- `to_live_risk_engine_config()` は Nautilus `LiveRiskEngineConfig` を組むだけ（ネイティブ rail）。
- `check_pre_trade()` / `check_post_trade()` は `RailViolation | None` を返すだけで、
  `OrderDenied` 生成・`SafetyRailViolation` push・run 停止は呼び出し側（exec client /
  controller / gRPC handler）の責務。

**0 = 無効（その rail を課さない）**: proto `SafetyLimits` の int は未指定で 0 になる。
backend は「0 はその rail 無効」と解釈し、実際の default 値（50万 / 5 / 100万 / 10万）は
Bevy 側 UI が供給する（§0.6「Default 値は Bevy 側で設定 UI を提供」= 構造的 bypass 不可は
backend が責任を持つが、数値ポリシーは UI 由来）。
"""

from __future__ import annotations

from dataclasses import dataclass

from nautilus_trader.live.config import LiveRiskEngineConfig


@dataclass(frozen=True)
class SafetyLimits:
    """proto `SafetyLimits` の transport 非依存ミラー。gRPC handler が proto から組む。"""

    max_position_size_jpy: int = 0
    max_order_value_jpy: int = 0
    max_daily_loss_jpy: int = 0
    max_orders_per_minute: int = 0
    allowed_instruments: tuple[str, ...] = ()


@dataclass(frozen=True)
class RailViolation:
    """Safety Rail 違反。`kind` は proto `SafetyRailViolation.kind` にそのまま載る。"""

    kind: str   # MAX_POSITION_SIZE / ALLOWED_INSTRUMENTS / MAX_DAILY_LOSS
    detail: str


# kind 定数（stringly-typed を避ける）。proto SafetyRailViolation.kind / UI トースト分類に使う。
KIND_MAX_POSITION_SIZE = "MAX_POSITION_SIZE"
KIND_ALLOWED_INSTRUMENTS = "ALLOWED_INSTRUMENTS"
KIND_MAX_DAILY_LOSS = "MAX_DAILY_LOSS"


class SafetyRails:
    """1 つの Live run の Safety Rails 評価器（純粋ロジック）。"""

    def __init__(self, limits: SafetyLimits) -> None:
        self._limits = limits

    @property
    def limits(self) -> SafetyLimits:
        return self._limits

    def to_live_risk_engine_config(
        self, instrument_ids: list[str]
    ) -> LiveRiskEngineConfig:
        """ネイティブ rail（`max_order_value` / `max_orders_per_minute`）を Nautilus に渡す
        `LiveRiskEngineConfig` に組む。

        - `max_order_value_jpy` → `max_notional_per_order`（instrument ごとに同額をマップ）。
          0 のときは何も入れない（= そのチェック無効、Nautilus 側 default 動作）。
        - `max_orders_per_minute` → `max_order_submit_rate = "N/00:01:00"`。0 のときは
          Nautilus default（`"100/00:00:01"`）に委ねる（フィールドを設定しない）。
        """
        kwargs: dict = {}
        if self._limits.max_order_value_jpy > 0:
            kwargs["max_notional_per_order"] = {
                iid: int(self._limits.max_order_value_jpy) for iid in instrument_ids
            }
        if self._limits.max_orders_per_minute > 0:
            kwargs["max_order_submit_rate"] = f"{self._limits.max_orders_per_minute}/00:01:00"
        return LiveRiskEngineConfig(**kwargs)

    def check_pre_trade(
        self,
        *,
        instrument_id: str,
        order_notional_jpy: float,
        current_position_value_jpy: float,
    ) -> RailViolation | None:
        """独自 pre-trade rail を評価する。違反なら `RailViolation`、OK なら `None`。

        呼び出し側（exec client）は違反時に `generate_order_denied()` + `SafetyRailViolation`
        push を行い、venue には送らない。

        - `allowed_instruments`: 空なら制限なし（起動時 instrument のみは gRPC 層が別途強制）。
          非空かつ instrument_id が含まれなければ違反。
        - `max_position_size_jpy`: |既存ポジション金額| + 新規注文金額 が上限超過なら違反
          （0 は無効）。建玉を増やす方向の保守的評価（§8 Open Risk 2、保守側に倒す）。
        """
        allowed = self._limits.allowed_instruments
        if allowed and instrument_id not in allowed:
            return RailViolation(
                KIND_ALLOWED_INSTRUMENTS,
                f"{instrument_id} not in allowed_instruments {list(allowed)}",
            )

        cap = self._limits.max_position_size_jpy
        if cap > 0:
            projected = abs(current_position_value_jpy) + abs(order_notional_jpy)
            if projected > cap:
                return RailViolation(
                    KIND_MAX_POSITION_SIZE,
                    f"projected position {projected:.0f} JPY exceeds cap {cap} JPY",
                )
        return None

    def check_post_trade(self, *, daily_pnl_jpy: float) -> RailViolation | None:
        """独自 post-trade rail（`max_daily_loss_jpy`）を評価する。

        当日の realized + unrealized P&L が `-max_daily_loss_jpy` を下回ったら違反。
        呼び出し側は run を `LiveStrategyStateMachine.error("MAX_DAILY_LOSS_EXCEEDED")` に
        遷移させ in-flight order を cancel する（§1.3）。0 は無効。
        """
        cap = self._limits.max_daily_loss_jpy
        if cap > 0 and daily_pnl_jpy < -cap:
            return RailViolation(
                KIND_MAX_DAILY_LOSS,
                f"daily P&L {daily_pnl_jpy:.0f} JPY breached loss limit -{cap} JPY",
            )
        return None
