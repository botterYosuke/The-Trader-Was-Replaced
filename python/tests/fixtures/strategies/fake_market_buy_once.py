"""Fake strategy that submits one market buy on the first bar.

Step 3B-1: fill 発生 → events.fills topic → write_fill の経路を固定する。

Scenario settings are specified via the ``fake_market_buy_once.json`` sidecar
(the ``scenario`` key).
"""

from __future__ import annotations

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


class FakeMarketBuyOnceConfig(StrategyConfig, frozen=True):
    instrument_id: str = "1301.TSE"
    bar_type: str = "1301.TSE-1-DAY-LAST-EXTERNAL"


class FakeMarketBuyOnce(Strategy):
    """Submits one market BUY on the first bar, then does nothing."""

    def __init__(self, config: FakeMarketBuyOnceConfig | None = None) -> None:
        super().__init__(config or FakeMarketBuyOnceConfig())
        self._submitted = False

    def on_start(self) -> None:
        self._instrument_id = InstrumentId.from_str(self.config.instrument_id)
        self.subscribe_bars(BarType.from_str(self.config.bar_type))

    def on_bar(self, bar: Bar) -> None:
        if self._submitted:
            return
        self._submitted = True
        order = self.order_factory.market(
            instrument_id=self._instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

    def on_stop(self) -> None:
        pass
