"""Minimal passthrough strategy for gRPC StartEngine integration tests (7203.TSE / Daily).

Does nothing; exists only to satisfy the strategy_file contract in StartEngine
and verify that the engine runs to completion and returns to IDLE.
"""
from __future__ import annotations

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.trading.strategy import Strategy

SCENARIO: dict = {
    "schema_version": 1,
    "instrument": "7203.TSE",
    "start": "2024-07-01",
    "end": "2024-07-02",
    "granularity": "Daily",
    "initial_cash": 1_000_000,
}


class PassthroughDailyStrategy(Strategy):
    """Receives bars and does nothing — minimum viable strategy for testing."""

    def __init__(
        self,
        *,
        instrument_id: str = "7203.TSE",
        bar_type_str: str | None = None,
        **_: object,
    ) -> None:
        super().__init__(config=StrategyConfig(strategy_id="passthrough-7203-daily"))
        self._bar_type_str = bar_type_str or f"{instrument_id}-1-DAY-LAST-EXTERNAL"

    def on_start(self) -> None:
        self.subscribe_bars(BarType.from_str(self._bar_type_str))

    def on_bar(self, bar: Bar) -> None:
        pass
