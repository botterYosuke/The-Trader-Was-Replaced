"""Minimal passthrough strategy for gRPC StartEngine integration tests (7203.TSE / Daily).

Does nothing; exists only to satisfy the strategy_file contract in StartEngine
and verify that the engine runs to completion and returns to IDLE.

Scenario settings are specified via the ``test_strategy_7203_daily.json`` sidecar
(the ``scenario`` key).
"""
from __future__ import annotations

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.trading.strategy import Strategy


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
