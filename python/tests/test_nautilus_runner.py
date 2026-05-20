"""
Tests for NautilusReplayRunner — runner layer between event source and DataEngine.
"""

from engine.core import DataEngine
from engine.nautilus_runner import NautilusReplayRunner, ReplayEventSink
from engine.reducer import KlineUpdate, ReplayTimeUpdated, TradeUpdate
from engine.replay import BaseReplayProvider


class _Price:
    def __init__(self, v: float):
        self._v = v

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


class _OneTickProvider(BaseReplayProvider):
    """Primes DataEngine at 1 ms so any positive test timestamp is valid."""

    def __init__(self):
        self._done = False

    def get_next_tick(self):
        if self._done:
            return None
        self._done = True
        return (0.001, 1.0, 1.0, 1.0, 1.0)

    def is_exhausted(self) -> bool:
        return self._done


def _engine() -> DataEngine:
    return DataEngine(replay_provider=_OneTickProvider())


# ---------------------------------------------------------------------------
# ReplayEventSink Protocol
# ---------------------------------------------------------------------------


def test_data_engine_satisfies_replay_event_sink_protocol():
    engine = _engine()
    # Static type check: DataEngine must structurally satisfy ReplayEventSink.
    sink: ReplayEventSink = engine  # type: ignore[assignment]
    assert hasattr(sink, "apply_replay_event")


# ---------------------------------------------------------------------------
# run_bars
# ---------------------------------------------------------------------------


def test_run_bars_updates_state_for_each_bar():
    engine = _engine()
    runner = NautilusReplayRunner(engine)

    bars = [
        _FakeBar(100, 110, 90, 105, ts_event_ns=10_000_000_000),
        _FakeBar(105, 115, 95, 112, ts_event_ns=20_000_000_000),
    ]
    runner.run_bars(bars)

    state = engine.get_current_state()
    assert state.timestamp_ms == 20_000
    assert state.close == 112.0
    assert state.price == 112.0


def test_run_bars_event_log_has_time_then_kline_pairs():
    engine = _engine()
    runner = NautilusReplayRunner(engine)

    bars = [
        _FakeBar(1, 1, 1, 10.0, ts_event_ns=5_000_000_000),
        _FakeBar(1, 1, 1, 20.0, ts_event_ns=10_000_000_000),
    ]
    runner.run_bars(bars)

    log = engine.get_event_log()
    # Primer tick is first; skip it, then check our two pairs.
    our_events = [e for e in log if not (isinstance(e, KlineUpdate) and e.close == 1.0)]
    assert len(our_events) == 4
    assert isinstance(our_events[0], ReplayTimeUpdated)
    assert isinstance(our_events[1], KlineUpdate)
    assert isinstance(our_events[2], ReplayTimeUpdated)
    assert isinstance(our_events[3], KlineUpdate)
    assert our_events[0].timestamp_ms == 5_000
    assert our_events[1].timestamp_ms == 5_000
    assert our_events[2].timestamp_ms == 10_000
    assert our_events[3].timestamp_ms == 10_000


def test_run_bars_empty_iterable_leaves_state_unchanged():
    engine = _engine()
    runner = NautilusReplayRunner(engine)
    state_before = engine.get_current_state()

    runner.run_bars([])

    state_after = engine.get_current_state()
    assert state_after.timestamp_ms == state_before.timestamp_ms
    assert state_after.price == state_before.price


# ---------------------------------------------------------------------------
# run_trades
# ---------------------------------------------------------------------------


def test_run_trades_updates_state_for_each_trade():
    engine = _engine()
    runner = NautilusReplayRunner(engine)

    trades = [
        _FakeTrade(price=3000.0, ts_event_ns=2_000_000_000),
        _FakeTrade(price=3050.0, ts_event_ns=4_000_000_000),
    ]
    runner.run_trades(trades)

    state = engine.get_current_state()
    assert state.timestamp_ms == 4_000
    assert state.price == 3050.0


def test_run_trades_event_log_has_time_then_trade_pairs():
    engine = _engine()
    runner = NautilusReplayRunner(engine)

    trades = [_FakeTrade(500.0, ts_event_ns=3_000_000_000)]
    runner.run_trades(trades)

    log = engine.get_event_log()
    our_events = [e for e in log if isinstance(e, (ReplayTimeUpdated, TradeUpdate))]
    # Filter out the primer KlineUpdate; ReplayTimeUpdated from primer has ts=1.
    trade_events = [e for e in our_events if not (isinstance(e, ReplayTimeUpdated) and e.timestamp_ms == 1)]
    assert len(trade_events) == 2
    assert isinstance(trade_events[0], ReplayTimeUpdated)
    assert isinstance(trade_events[1], TradeUpdate)
    assert trade_events[0].timestamp_ms == 3_000
    assert trade_events[1].timestamp_ms == 3_000


# ---------------------------------------------------------------------------
# ordering across mixed bar + trade sequence
# ---------------------------------------------------------------------------


def test_bars_then_trades_advance_time_monotonically():
    engine = _engine()
    runner = NautilusReplayRunner(engine)

    runner.run_bars([_FakeBar(1, 1, 1, 200.0, ts_event_ns=5_000_000_000)])
    runner.run_trades([_FakeTrade(210.0, ts_event_ns=6_000_000_000)])

    state = engine.get_current_state()
    assert state.timestamp_ms == 6_000
    assert state.price == 210.0
