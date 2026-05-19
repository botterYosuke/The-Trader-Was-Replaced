"""
Nautilus event → reducer event conversion layer.

Accepts fake/duck-typed objects for now; attribute names match real Nautilus
Bar and TradeTick types so the same functions work once the real import is wired.

Nautilus Bar attributes used:
  bar.open.as_double()  bar.high.as_double()  bar.low.as_double()
  bar.close.as_double()
  bar.ts_event           (nanoseconds, int) — bar close time, used as primary ts
  bar.ts_init            (nanoseconds, int) — ingestion time, unused here

Nautilus TradeTick attributes used:
  trade.price.as_double()
  trade.ts_event         (nanoseconds, int)
"""

from .reducer import KlineUpdate, ReplayTimeUpdated, TradeUpdate


def _ns_to_ms(nanoseconds: int) -> int:
    return nanoseconds // 1_000_000


def bar_to_kline_update(bar, instrument_id: str = "") -> KlineUpdate:
    ts_ms = _ns_to_ms(bar.ts_event)
    return KlineUpdate(
        timestamp_ms=ts_ms,
        open_time_ms=ts_ms,
        open=bar.open.as_double(),
        high=bar.high.as_double(),
        low=bar.low.as_double(),
        close=bar.close.as_double(),
        instrument_id=instrument_id,
    )


def trade_to_trade_update(trade) -> TradeUpdate:
    return TradeUpdate(
        timestamp_ms=_ns_to_ms(trade.ts_event),
        price=trade.price.as_double(),
    )


def timestamp_ns_to_replay_time_updated(ts_ns: int) -> ReplayTimeUpdated:
    return ReplayTimeUpdated(timestamp_ms=_ns_to_ms(ts_ns))
