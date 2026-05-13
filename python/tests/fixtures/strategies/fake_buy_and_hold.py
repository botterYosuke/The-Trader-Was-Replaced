"""Fake buy-and-hold strategy fixture for engine_runner Step 3A tests.

SCENARIO は最小構成 (単一銘柄 / Daily / initial_cash)。
策略は bar を subscribe するだけで注文を出さない。
"""

from __future__ import annotations

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.trading.strategy import Strategy

SCENARIO = {
    "schema_version": 1,
    "instrument": "1301.TSE",
    "granularity": "Daily",
    "start": "2024-01-01",
    "end": "2024-12-31",
    "initial_cash": 10_000_000,
}


class FakeBuyAndHoldConfig(StrategyConfig, frozen=True):
    instrument_id: str = "1301.TSE"
    bar_type: str = "1301.TSE-1-DAY-LAST-EXTERNAL"
    bar_types: list[str] | None = None  # 複数銘柄用: None なら bar_type の単一リストを使う


class FakeBuyAndHold(Strategy):
    """Receives bars but places no orders. Step 3A smoke test strategy."""

    def __init__(self, config: FakeBuyAndHoldConfig | None = None) -> None:
        super().__init__(config or FakeBuyAndHoldConfig())

    def on_start(self) -> None:
        bar_types = self.config.bar_types or [self.config.bar_type]
        for bt in bar_types:
            self.subscribe_bars(BarType.from_str(bt))

    def on_bar(self, bar: Bar) -> None:
        pass

    def on_stop(self) -> None:
        pass
