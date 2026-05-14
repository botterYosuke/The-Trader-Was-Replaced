"""最小限の passthrough 戦略 — gRPC StartEngine 統合テスト用 (7203.TSE / Minute)。

何もしない戦略で、Minute 粒度の StartEngine contract をテストするためだけに使う。
"""
from __future__ import annotations

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.trading.strategy import Strategy

SCENARIO: dict = {
    "schema_version": 1,
    "instrument": "7203.TSE",
    "start": "2024-07-01",
    "end": "2024-07-01",
    "granularity": "Minute",
    "initial_cash": 1_000_000,
}


class PassthroughMinuteStrategy(Strategy):
    """バーを受信するだけで何もしない最小戦略（Minute 粒度）。"""

    def __init__(
        self,
        *,
        instrument_id: str = "7203.TSE",
        bar_type_str: str | None = None,
        **_: object,
    ) -> None:
        super().__init__(config=StrategyConfig(strategy_id="passthrough-7203-minute"))
        self._bar_type_str = bar_type_str or f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"

    def on_start(self) -> None:
        self.subscribe_bars(BarType.from_str(self._bar_type_str))

    def on_bar(self, bar: Bar) -> None:
        pass
