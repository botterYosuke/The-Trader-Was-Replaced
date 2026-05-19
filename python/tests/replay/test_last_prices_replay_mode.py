"""Tests for GetState last_prices in Replay mode (D8).

Verifies that engine.get_replay_last_prices() returns per-instrument close
prices populated by the multi-instrument _advance_one_locked (D9/D24).
"""
from __future__ import annotations

import pytest

from engine.core import DataEngine
from engine.reducer import ReducerState
from engine.models import HistoryPoint, OhlcPoint


def _stub_engine_with_primed(close_map: dict[str, float]) -> DataEngine:
    """Create a DataEngine with per_id_close pre-populated (simulates post-advance state)."""
    engine = DataEngine()
    ts_ms = 5000
    c = next(iter(close_map.values()))
    engine._rs = ReducerState(
        timestamp_ms=ts_ms,
        price=c,
        history=[c],
        history_points=[HistoryPoint(timestamp_ms=ts_ms, price=c)],
        ohlc_points=[OhlcPoint(timestamp_ms=ts_ms, open_time_ms=ts_ms, open=c, high=c, low=c, close=c)],
        max_history_len=1000,
    )
    for iid, price in close_map.items():
        engine._rs.per_id_close[iid] = price
    return engine


def test_get_replay_last_prices_returns_empty_when_no_advances():
    """Before any advances, per_id_close should be empty."""
    engine = DataEngine()
    prices = engine.get_replay_last_prices()
    assert prices == {}


def test_get_replay_last_prices_includes_all_instruments():
    """get_replay_last_prices returns all instruments in per_id_close."""
    engine = _stub_engine_with_primed({"1301.TSE": 2500.0, "7203.TSE": 8000.0})
    prices = engine.get_replay_last_prices()
    assert prices == {"1301.TSE": 2500.0, "7203.TSE": 8000.0}


def test_get_replay_last_prices_returns_independent_copy():
    """Mutating the returned dict must not affect internal state."""
    engine = _stub_engine_with_primed({"1301.TSE": 2500.0})
    prices1 = engine.get_replay_last_prices()
    prices1["1301.TSE"] = 9999.0
    prices2 = engine.get_replay_last_prices()
    assert prices2["1301.TSE"] == pytest.approx(2500.0)
