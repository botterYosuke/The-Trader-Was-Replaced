"""Tests for nautilus_adapter using fake duck-typed objects."""

from engine.nautilus_adapter import (
    bar_to_kline_update,
    timestamp_ns_to_replay_time_updated,
    trade_to_trade_update,
)
from engine.reducer import KlineUpdate, ReplayTimeUpdated, TradeUpdate


class _Price:
    def __init__(self, value: float):
        self._v = value

    def as_double(self) -> float:
        return self._v


class _Time:
    def __init__(self, ns: int):
        self.value = ns


class _FakeBar:
    def __init__(self, open, high, low, close, ts_event_ns: int, ts_init_ns: int = 0, volume: float = 0.0):
        self.open = _Price(open)
        self.high = _Price(high)
        self.low = _Price(low)
        self.close = _Price(close)
        self.volume = _Price(volume)
        self.ts_event = ts_event_ns
        self.ts_init = ts_init_ns


class _FakeTrade:
    def __init__(self, price: float, ts_event_ns: int):
        self.price = _Price(price)
        self.ts_event = ts_event_ns


# ---------------------------------------------------------------------------
# bar_to_kline_update
# ---------------------------------------------------------------------------


def test_bar_to_kline_update_fields():
    bar = _FakeBar(
        open=100.0,
        high=110.0,
        low=90.0,
        close=105.0,
        ts_event_ns=60_000_000_000,  # 60 seconds → 60000 ms
    )
    result = bar_to_kline_update(bar)

    assert isinstance(result, KlineUpdate)
    assert result.open == 100.0
    assert result.high == 110.0
    assert result.low == 90.0
    assert result.close == 105.0
    assert result.timestamp_ms == 60_000
    assert result.open_time_ms == result.timestamp_ms


def test_bar_to_kline_update_ns_truncation():
    # 1_500_000_999 ns → should be 1_500 ms (truncate, not round)
    bar = _FakeBar(1, 1, 1, 1, ts_event_ns=1_500_000_999)
    result = bar_to_kline_update(bar)
    assert result.timestamp_ms == 1_500


# ---------------------------------------------------------------------------
# trade_to_trade_update
# ---------------------------------------------------------------------------


def test_trade_to_trade_update_fields():
    trade = _FakeTrade(price=3000.5, ts_event_ns=5_000_000_000)
    result = trade_to_trade_update(trade)

    assert isinstance(result, TradeUpdate)
    assert result.price == 3000.5
    assert result.timestamp_ms == 5_000


def test_trade_to_trade_update_zero_timestamp():
    trade = _FakeTrade(price=1.0, ts_event_ns=0)
    result = trade_to_trade_update(trade)
    assert result.timestamp_ms == 0


# ---------------------------------------------------------------------------
# timestamp_ns_to_replay_time_updated
# ---------------------------------------------------------------------------


def test_timestamp_ns_to_replay_time_updated():
    result = timestamp_ns_to_replay_time_updated(120_000_000_000)
    assert isinstance(result, ReplayTimeUpdated)
    assert result.timestamp_ms == 120_000


def test_timestamp_ns_to_replay_time_updated_zero():
    result = timestamp_ns_to_replay_time_updated(0)
    assert result.timestamp_ms == 0
