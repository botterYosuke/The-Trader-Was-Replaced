"""
Integration tests for the adapter → DataEngine pipeline.

Nautilus object → nautilus_adapter → ReplayEvent → DataEngine.apply_replay_event()
→ ReducerState → get_current_state()
"""

from engine.core import DataEngine
from engine.nautilus_adapter import (
    bar_to_kline_update,
    timestamp_ns_to_replay_time_updated,
    trade_to_trade_update,
)
from engine.reducer import KlineUpdate, ReplayTimeUpdated
from engine.replay import BaseReplayProvider


class _Price:
    def __init__(self, value: float):
        self._v = value

    def as_double(self) -> float:
        return self._v


class _FakeBar:
    def __init__(self, open, high, low, close, ts_event_ns: int, volume: float = 0.0):
        self.open = _Price(open)
        self.high = _Price(high)
        self.low = _Price(low)
        self.close = _Price(close)
        self.volume = _Price(volume)
        self.ts_event = ts_event_ns
        self.ts_init = 0


class _FakeTrade:
    def __init__(self, price: float, ts_event_ns: int):
        self.price = _Price(price)
        self.ts_event = ts_event_ns


class _ZeroProvider(BaseReplayProvider):
    """Single-tick provider that starts the engine at timestamp_ms=0."""

    def __init__(self):
        self._done = False

    def get_next_tick(self):
        if self._done:
            return None
        self._done = True
        return (0.001, 1.0, 1.0, 1.0, 1.0)  # 1 ms start — HistoryPoint requires timestamp_ms > 0

    def is_exhausted(self) -> bool:
        return self._done


def _zeroed_engine() -> DataEngine:
    """DataEngine primed from timestamp_ms=0 so any positive test timestamp is valid."""
    return DataEngine(replay_provider=_ZeroProvider())


# ---------------------------------------------------------------------------
# bar_to_kline_update → apply_replay_event → get_current_state
# ---------------------------------------------------------------------------


def test_bar_event_updates_state_via_apply_replay_event():
    engine = _zeroed_engine()
    bar = _FakeBar(open=100.0, high=110.0, low=90.0, close=105.0, ts_event_ns=60_000_000_000)

    ts_event = timestamp_ns_to_replay_time_updated(bar.ts_event)
    kline_event = bar_to_kline_update(bar)

    engine.apply_replay_event(ts_event)
    engine.apply_replay_event(kline_event)

    state = engine.get_current_state()
    assert state.timestamp_ms == kline_event.timestamp_ms
    assert state.close == kline_event.close
    assert state.price == kline_event.close
    assert state.open == kline_event.open
    assert state.high == kline_event.high
    assert state.low == kline_event.low


def test_trade_event_updates_state_via_apply_replay_event():
    engine = _zeroed_engine()
    trade = _FakeTrade(price=3000.5, ts_event_ns=5_000_000_000)

    ts_event = timestamp_ns_to_replay_time_updated(trade.ts_event)
    trade_event = trade_to_trade_update(trade)

    engine.apply_replay_event(ts_event)
    engine.apply_replay_event(trade_event)

    state = engine.get_current_state()
    assert state.timestamp_ms == trade_event.timestamp_ms
    assert state.price == trade_event.price


# ---------------------------------------------------------------------------
# event_log ordering
# ---------------------------------------------------------------------------


def test_event_log_preserves_replay_time_then_kline_order():
    engine = _zeroed_engine()
    bar = _FakeBar(open=200.0, high=210.0, low=195.0, close=205.0, ts_event_ns=120_000_000_000)

    ts_event = timestamp_ns_to_replay_time_updated(bar.ts_event)
    kline_event = bar_to_kline_update(bar)

    engine.apply_replay_event(ts_event)
    engine.apply_replay_event(kline_event)

    log = engine.get_event_log()
    # Only care about the two events we just injected (static engine may have prior events).
    recent = log[-2:]
    assert isinstance(recent[0], ReplayTimeUpdated)
    assert isinstance(recent[1], KlineUpdate)
    assert recent[0].timestamp_ms == ts_event.timestamp_ms
    assert recent[1].timestamp_ms == kline_event.timestamp_ms


# ---------------------------------------------------------------------------
# stale event is ignored through DataEngine
# ---------------------------------------------------------------------------


def test_stale_kline_ignored_via_apply_replay_event():
    engine = _zeroed_engine()

    future_bar = _FakeBar(1, 1, 1, 999.0, ts_event_ns=100_000_000_000)
    past_bar = _FakeBar(1, 1, 1, 1.0, ts_event_ns=1_000_000_000)

    engine.apply_replay_event(bar_to_kline_update(future_bar))
    state_before = engine.get_current_state()

    engine.apply_replay_event(bar_to_kline_update(past_bar))
    state_after = engine.get_current_state()

    assert state_after.price == state_before.price
    assert state_after.timestamp_ms == state_before.timestamp_ms
