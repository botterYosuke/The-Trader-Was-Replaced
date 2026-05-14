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


def test_kline_update_stores_ohlc_in_state():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=105.0, open=101.0, high=108.0, low=99.0, open_time_ms=1900))
    assert state.open == 101.0
    assert state.high == 108.0
    assert state.low == 99.0
    assert state.open_time_ms == 1900


def test_kline_update_without_ohlc_leaves_zero_defaults():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=105.0))
    assert state.open == 0.0
    assert state.high == 0.0
    assert state.low == 0.0
    assert state.open_time_ms == 0


def test_kline_update_ohlc_not_stored_for_stale_event():
    state = _state(timestamp_ms=5000, price=100.0)
    state.open = 50.0
    apply_event(state, KlineUpdate(timestamp_ms=4999, close=999.0, open=1.0, high=2.0, low=3.0))
    assert state.open == 50.0


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


# --- ordering / same-timestamp policy ---
# Policy: last-event-wins; stale (ts < state.timestamp_ms) is silently ignored;
# equal ts is accepted (not stale).

def test_time_updated_then_trade_update_accepted_at_same_ts():
    state = _state(timestamp_ms=1000, price=100.0, history=[100.0], history_points=[])
    apply_event(state, ReplayTimeUpdated(timestamp_ms=2000))
    apply_event(state, TradeUpdate(timestamp_ms=2000, price=110.0))
    assert state.price == 110.0
    assert state.timestamp_ms == 2000


def test_kline_then_trade_same_ts_trade_wins():
    """後着の TradeUpdate が同一 ts で KlineUpdate を上書きする。"""
    state = _state(timestamp_ms=0, price=0.0, history=[], history_points=[])
    apply_event(state, KlineUpdate(timestamp_ms=1000, close=100.0))
    apply_event(state, TradeUpdate(timestamp_ms=1000, price=101.0))
    assert state.price == 101.0
    assert state.timestamp_ms == 1000
    assert state.history == [100.0, 101.0]


def test_trade_then_kline_same_ts_kline_wins():
    """後着の KlineUpdate が同一 ts で TradeUpdate を上書きする。"""
    state = _state(timestamp_ms=0, price=0.0, history=[], history_points=[])
    apply_event(state, TradeUpdate(timestamp_ms=1000, price=101.0))
    apply_event(state, KlineUpdate(timestamp_ms=1000, close=100.0))
    assert state.price == 100.0
    assert state.timestamp_ms == 1000
    assert state.history == [101.0, 100.0]


def test_stale_trade_after_kline_ignored():
    """KlineUpdate より古い TradeUpdate は無視される。"""
    state = _state(timestamp_ms=0, price=0.0, history=[], history_points=[])
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=100.0))
    apply_event(state, TradeUpdate(timestamp_ms=1999, price=999.0))
    assert state.price == 100.0
    assert state.history == [100.0]


def test_multiple_trade_updates_same_ts_all_appended():
    """同一 ts の複数 TradeUpdate はすべて history に積まれる。"""
    state = _state(timestamp_ms=0, price=0.0, history=[], history_points=[])
    apply_event(state, TradeUpdate(timestamp_ms=1000, price=100.0))
    apply_event(state, TradeUpdate(timestamp_ms=1000, price=101.0))
    apply_event(state, TradeUpdate(timestamp_ms=1000, price=102.0))
    assert state.price == 102.0
    assert state.history == [100.0, 101.0, 102.0]
    assert len(state.history_points) == 3


def test_kline_ohlc_not_overwritten_by_subsequent_trade():
    """TradeUpdate が来ても KlineUpdate の OHLC フィールドは変わらない。"""
    state = _state(timestamp_ms=0, price=0.0)
    apply_event(state, KlineUpdate(timestamp_ms=1000, close=100.0, open=95.0, high=105.0, low=93.0))
    apply_event(state, TradeUpdate(timestamp_ms=1000, price=101.0))
    assert state.open == 95.0
    assert state.high == 105.0
    assert state.low == 93.0
    assert state.price == 101.0


def test_replay_time_updated_then_stale_kline_ignored():
    """ReplayTimeUpdated で時刻が進んだ後、古い KlineUpdate は無視される。"""
    state = _state(timestamp_ms=0, price=50.0, history=[50.0], history_points=[])
    apply_event(state, ReplayTimeUpdated(timestamp_ms=3000))
    apply_event(state, KlineUpdate(timestamp_ms=2999, close=999.0))
    assert state.price == 50.0
    assert state.timestamp_ms == 3000
    assert state.history == [50.0]


# --- ohlc_points ---

def test_kline_update_appends_ohlc_point():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=105.0, open=101.0, high=108.0, low=99.0, open_time_ms=1900))
    assert len(state.ohlc_points) == 1
    pt = state.ohlc_points[0]
    assert pt.timestamp_ms == 2000
    assert pt.open_time_ms == 1900
    assert pt.open == 101.0
    assert pt.high == 108.0
    assert pt.low == 99.0
    assert pt.close == 105.0


def test_kline_update_uses_timestamp_ms_when_open_time_ms_zero():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, KlineUpdate(timestamp_ms=2000, close=105.0, open=101.0, high=108.0, low=99.0, open_time_ms=0))
    assert state.ohlc_points[0].open_time_ms == 2000


def test_trade_update_does_not_append_ohlc_points():
    state = _state(timestamp_ms=1000, price=100.0)
    apply_event(state, TradeUpdate(timestamp_ms=2000, price=110.0))
    assert state.ohlc_points == []


def test_kline_update_trims_ohlc_points_at_max_len():
    state = _state(timestamp_ms=0, price=100.0, max_history_len=3)
    for i in range(1, 6):
        apply_event(state, KlineUpdate(timestamp_ms=i * 1000, close=100.0 + i, open=100.0, high=101.0 + i, low=99.0, open_time_ms=i * 1000))
    assert len(state.ohlc_points) == 3
    assert state.ohlc_points[-1].close == 105.0


def test_multiple_klines_build_ordered_ohlc_history():
    state = _state(timestamp_ms=0, price=10.0)
    bars = [(1000, 10.0, 9.5, 10.5, 9.0), (2000, 20.0, 19.0, 21.0, 18.0), (3000, 30.0, 29.0, 31.0, 28.0)]
    for ts, close, open_, high, low in bars:
        apply_event(state, KlineUpdate(timestamp_ms=ts, close=close, open=open_, high=high, low=low, open_time_ms=ts - 100))
    assert len(state.ohlc_points) == 3
    assert [pt.close for pt in state.ohlc_points] == [10.0, 20.0, 30.0]


def test_stale_kline_does_not_append_ohlc_point():
    state = _state(timestamp_ms=5000, price=100.0)
    apply_event(state, KlineUpdate(timestamp_ms=4999, close=999.0, open=900.0, high=1000.0, low=800.0))
    assert state.ohlc_points == []
