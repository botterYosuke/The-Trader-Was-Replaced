"""engine.strategy_runtime.instrument_factory — Equity Instrument 生成。

e-station の engine.nautilus.instrument_factory を最小移植。
InstrumentCache 依存を除去し、固定値を使う (price_precision=1, lot_size=100)。
"""

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.identifiers import InstrumentId, Symbol, Venue
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Price, Quantity


def make_equity_instrument(symbol: str, venue: str) -> Equity:
    """TSE 上場銘柄の Equity を生成する。

    price_precision=1 / price_increment=0.1 / lot_size=100 固定。
    ts_event / ts_init は 0 (バックテスト用 N0 仮置き)。
    """
    return Equity(
        instrument_id=InstrumentId(Symbol(symbol), Venue(venue)),
        raw_symbol=Symbol(symbol),
        currency=JPY,
        price_precision=1,
        price_increment=Price(Decimal("0.1"), precision=1),
        lot_size=Quantity(100, precision=0),
        isin=None,
        ts_event=0,
        ts_init=0,
    )
