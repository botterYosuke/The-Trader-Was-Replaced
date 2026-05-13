from engine.reducer import (
    KlineUpdate,
    ReducerState,
    ReplayTimeUpdated,
    TradeUpdate,
    apply_event,
)


def _state(**kwargs) -> ReducerState:
    defaults = dict(timestamp_ms=1000000, price=100.0, history=[100.0], history_points=[], max_history_len=1000)
    defaults.update(kwargs)
    return ReducerState(**defaults)


# --- ReplayTimeUpdated ---

def test_replay_time_updated_advances_timestamp():
    state = _state(timestamp_ms=1000)
    apply_event(state, ReplayTimeUpdated(timestamp_ms=2000))
    assert state.timestamp_ms == 2000


def test_replay_time_updated_does_not_change_price_or_history():
    state = _state(timestamp_ms=1000, price=42.0, history=[42.0])
    apply_event(state, ReplayTimeUpdated(timestamp_ms=2000))
    assert state.price == 42.0
    assert state.history == [42.0]


def test_replay_time_updated_ignores_stale_timestamp():
    state = _state(timestamp_ms=5000)
    apply_event(state, ReplayTimeUpdated(timestamp_ms=4999))
    assert state.timestamp_ms == 5000


def test_replay_time_updated_accepts_equal_timestamp():
    state = _state(timestamp_ms=5000)
    apply_event(state, ReplayTimeUpdated(timestamp_ms=5000))
    assert state.timestamp_ms == 5000


# --- KlineUpdate ---

def test_kline_update_advances_price_and_timestamp():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=105.0))
    assert state.price == 105.0
    assert state.timestamp_ms == 2000


def test_kline_update_appends_to_history():
    state = _state(timestamp_ms=1000, price=100.0, history=[100.0], history_points=[])
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=105.0))
    assert state.history == [100.0, 105.0]
    assert len(state.history_points) == 1
    assert state.history_points[0].timestamp_ms == 2000
    assert state.history_points[0].price == 105.0


def test_kline_update_ignores_stale_timestamp():
    state = _state(timestamp_ms=5000, price=100.0, history=[100.0])
    apply_event(state, KlineUpdate(timestamp_ms=4999, close=999.0))
    assert state.price == 100.0
    assert state.history == [100.0]
    assert state.timestamp_ms == 5000


def _make_history_points(prices: list, base_ts: int = 1):
    from engine.models import HistoryPoint
    return [HistoryPoint(timestamp_ms=base_ts + i, price=p) for i, p in enumerate(prices)]


def test_kline_update_trims_history_at_max_len():
    prices = [1.0, 2.0]
    state = _state(timestamp_ms=2, price=2.0, history=list(prices),
                   history_points=_make_history_points(prices), max_history_len=3)
    apply_event(state, KlineUpdate(timestamp_ms=3, close=3.0))
    apply_event(state, KlineUpdate(timestamp_ms=4, close=4.0))
    assert len(state.history) == 3
    assert state.history == [2.0, 3.0, 4.0]


def test_kline_update_trims_history_points_in_sync():
    prices = [1.0, 2.0]
    state = _state(timestamp_ms=2, price=2.0, history=list(prices),
                   history_points=_make_history_points(prices), max_history_len=3)
    apply_event(state, KlineUpdate(timestamp_ms=3, close=3.0))
    apply_event(state, KlineUpdate(timestamp_ms=4, close=4.0))
    assert len(state.history_points) == len(state.history)
    assert state.history_points[-1].price == state.history[-1]


# --- TradeUpdate ---

def test_trade_update_advances_price_and_timestamp():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, TradeUpdate(timestamp_ms=2000, price=110.0))
    assert state.price == 110.0
    assert state.timestamp_ms == 2000


def test_trade_update_appends_to_history():
    state = _state(timestamp_ms=1000, price=100.0, history=[100.0], history_points=[])
    apply_event(state, TradeUpdate(timestamp_ms=2000, price=110.0))
    assert state.history == [100.0, 110.0]
    assert state.history_points[0].price == 110.0


def test_trade_update_ignores_stale_timestamp():
    state = _state(timestamp_ms=5000, price=100.0, history=[100.0])
    apply_event(state, TradeUpdate(timestamp_ms=4999, price=999.0))
    assert state.price == 100.0


# --- sequence ---

def test_time_updated_then_kline_update_uses_kline_timestamp():
    state = _state(timestamp_ms=1000, price=100.0, history=[100.0], history_points=[])
    apply_event(state, ReplayTimeUpdated(timestamp_ms=2000))
    apply_event(state, KlineUpdate(timestamp_ms=3000, close=200.0))
    assert state.timestamp_ms == 3000
    assert state.price == 200.0


def test_kline_then_time_updated_does_not_rewind():
    state = _state(timestamp_ms=1000, price=100.0, history=[100.0], history_points=[])
    apply_event(state, KlineUpdate(timestamp_ms=3000, close=200.0))
    apply_event(state, ReplayTimeUpdated(timestamp_ms=2000))
    assert state.timestamp_ms == 3000


def test_multiple_klines_build_ordered_history():
    state = _state(timestamp_ms=0, price=10.0, history=[], history_points=[])
    prices = [10.0, 20.0, 30.0]
    for i, p in enumerate(prices):
        apply_event(state, KlineUpdate(timestamp_ms=(i + 1) * 1000, close=p))
    assert state.history == prices
    assert [hp.price for hp in state.history_points] == prices
    assert state.history_points[-1].timestamp_ms == 3000
