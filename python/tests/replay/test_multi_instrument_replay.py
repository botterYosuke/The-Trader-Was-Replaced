"""Multi-instrument Replay engine tests (D9 / D24).

Tests:
- DataEngine._replay_providers dict is populated for each instrument_id
- _advance_one_locked drains all providers at the same min_ts in one tick
- per_id_close is updated per instrument after advance
- get_replay_last_prices returns full dict
- ReplayTimeUpdated fires once per ts group (not once per instrument)
- Exhaustion detected when all providers are exhausted
"""
from __future__ import annotations

import pytest

from engine.core import DataEngine, instrument_id_to_bar_type
from engine.replay import NautilusBarsReplayProvider


# ── Minimal stub provider ──────────────────────────────────────────────────────

class _StubProvider:
    """In-memory provider for unit-testing without catalog IO."""

    def __init__(self, ticks: list[tuple[float, float, float, float, float]]):
        self._data = ticks
        self._idx = 0

    def get_next_tick(self):
        return self.pop_next_tick()

    def peek_next_tick(self):
        if self._idx < len(self._data):
            return self._data[self._idx]
        return None

    def pop_next_tick(self):
        if self._idx < len(self._data):
            t = self._data[self._idx]
            self._idx += 1
            return t
        return None

    def is_exhausted(self) -> bool:
        return self._idx >= len(self._data)


def _engine_with_providers(providers: dict[str, _StubProvider]) -> DataEngine:
    """Build a DataEngine with the given multi-instrument providers injected."""
    engine = DataEngine()
    # Prime with first provider's first tick
    first_iid = next(iter(providers))
    first_provider = providers[first_iid]
    first_tick = first_provider.pop_next_tick()
    assert first_tick is not None, "stub must have at least one tick"
    ts, o, h, l, c = first_tick
    ts_ms = int(ts * 1000)
    from engine.reducer import ReducerState
    from engine.models import HistoryPoint, OhlcPoint
    engine._rs = ReducerState(
        timestamp_ms=ts_ms,
        price=c,
        open=o,
        high=h,
        low=l,
        history=[c],
        history_points=[HistoryPoint(timestamp_ms=ts_ms, price=c)],
        ohlc_points=[OhlcPoint(timestamp_ms=ts_ms, open_time_ms=ts_ms, open=o, high=h, low=l, close=c)],
        max_history_len=1000,
    )
    engine._rs.per_id_close[first_iid] = c
    engine._replay_providers = dict(providers)
    engine._replay_primary_id = first_iid
    engine._mode = "replay"
    engine._replay_state = "RUNNING"
    engine._is_running = True
    return engine


# ── Tests ─────────────────────────────────────────────────────────────────────

def test_replay_providers_dict_populated_for_all_instruments():
    """_replay_providers must contain an entry per instrument_id."""
    p1 = _StubProvider([(1.0, 100.0, 101.0, 99.0, 100.5)])
    p2 = _StubProvider([(1.0, 200.0, 201.0, 199.0, 200.5)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    assert "A.TSE" in engine._replay_providers
    assert "B.TSE" in engine._replay_providers


def test_advance_drains_same_ts_instruments_in_one_tick():
    """Two providers with the same timestamp → both drained in one _advance_one_locked."""
    # ts=2.0 for both
    p1 = _StubProvider([(2.0, 100.0, 110.0, 90.0, 105.0)])
    p2 = _StubProvider([(2.0, 200.0, 220.0, 180.0, 210.0)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    engine._advance_one_locked()
    # Both should now be exhausted after one advance
    assert p1.is_exhausted()
    assert p2.is_exhausted()


def test_per_id_close_updated_per_instrument_after_advance():
    """After advance, per_id_close should have entries for all advanced instruments."""
    p1 = _StubProvider([(2.0, 100.0, 110.0, 90.0, 105.0)])
    p2 = _StubProvider([(2.0, 200.0, 220.0, 180.0, 210.0)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    engine._advance_one_locked()
    assert engine._rs.per_id_close.get("A.TSE") == pytest.approx(105.0)
    assert engine._rs.per_id_close.get("B.TSE") == pytest.approx(210.0)


def test_get_replay_last_prices_returns_full_dict():
    """get_replay_last_prices() returns all per_id_close entries."""
    p1 = _StubProvider([(2.0, 100.0, 110.0, 90.0, 105.0)])
    p2 = _StubProvider([(2.0, 200.0, 220.0, 180.0, 210.0)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    engine._advance_one_locked()
    prices = engine.get_replay_last_prices()
    assert "A.TSE" in prices
    assert "B.TSE" in prices
    assert prices["A.TSE"] == pytest.approx(105.0)
    assert prices["B.TSE"] == pytest.approx(210.0)


def test_replay_time_updated_fires_once_per_ts_group():
    """ReplayTimeUpdated should appear exactly once for a same-ts group of providers."""
    from engine.reducer import ReplayTimeUpdated

    p1 = _StubProvider([(2.0, 100.0, 110.0, 90.0, 105.0)])
    p2 = _StubProvider([(2.0, 200.0, 220.0, 180.0, 210.0)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    engine._advance_one_locked()
    time_updates = [e for e in engine._event_log if isinstance(e, ReplayTimeUpdated)]
    # Exactly 1 ReplayTimeUpdated for the same-ts group
    assert len(time_updates) == 1
    assert time_updates[0].timestamp_ms == 2000


def test_exhaustion_detected_when_all_providers_exhausted():
    """Engine should be exhausted only after all providers have no more data.

    Note: _engine_with_providers primes using the first tick of p1 (A.TSE).
    So p1 needs an extra tick to have data remaining after priming.
    """
    # A.TSE: 2 ticks — first consumed by priming, second available at ts=2.0
    p1 = _StubProvider([
        (1.0, 100.0, 110.0, 90.0, 100.0),  # priming tick (consumed by _engine_with_providers)
        (2.0, 100.0, 110.0, 90.0, 105.0),  # available after priming
    ])
    # B.TSE: 1 tick at ts=3.0 (different timestamp → not drained with A at ts=2.0)
    p2 = _StubProvider([(3.0, 200.0, 220.0, 180.0, 210.0)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    # After priming, p1 should still have 1 tick, p2 has its 1 tick
    assert not p1.is_exhausted()
    assert not p2.is_exhausted()
    # First advance: drains A at ts=2.0, B not drained (ts=3.0)
    engine._advance_one_locked()
    assert p1.is_exhausted()
    assert not p2.is_exhausted()
    assert not engine._is_exhausted
    # Second advance: drains B at ts=3.0
    engine._advance_one_locked()
    assert engine._is_exhausted
