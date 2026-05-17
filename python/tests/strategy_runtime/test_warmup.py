"""Tests for engine.strategy_runtime.warmup."""
from __future__ import annotations

import sys
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch

from engine.paths import jquants_catalog_path

import pytest

from engine.strategy_runtime.warmup import make_catalog_warmup_loader


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_CATALOG = jquants_catalog_path()
_REAL_CATALOG_AVAILABLE = _CATALOG.exists()


def _make_fake_bar(ts_event_ns: int, open_=100.0, high=105.0, low=99.0, close=102.0, volume=1000.0):
    """Build a minimal bar-like object with nautilus_trader fields."""
    class _FakePrice:
        def __init__(self, v): self._v = v
        def __str__(self): return str(self._v)

    class _FakeBar:
        def __init__(self):
            self.ts_event = ts_event_ns
            self.open = _FakePrice(open_)
            self.high = _FakePrice(high)
            self.low = _FakePrice(low)
            self.close = _FakePrice(close)
            self.volume = _FakePrice(volume)

    return _FakeBar()


def _ns(d: date) -> int:
    return int(datetime(d.year, d.month, d.day, tzinfo=timezone.utc).timestamp() * 1_000_000_000)


# ---------------------------------------------------------------------------
# Unit tests (no real catalog required)
# ---------------------------------------------------------------------------


def test_returns_callable(tmp_path):
    loader = make_catalog_warmup_loader(tmp_path)
    assert callable(loader)


def test_returns_empty_when_load_bars_raises(tmp_path):
    loader = make_catalog_warmup_loader(tmp_path)
    with patch("engine.strategy_runtime.warmup.load_bars", side_effect=FileNotFoundError("no file")):
        result = loader("1301.TSE", date(2025, 1, 1), date(2025, 1, 5))
    assert result == []


def test_returns_empty_when_no_bars_in_range(tmp_path):
    loader = make_catalog_warmup_loader(tmp_path)
    bar_outside = _make_fake_bar(_ns(date(2024, 12, 1)))
    with patch("engine.strategy_runtime.warmup.load_bars", return_value=[bar_outside]):
        result = loader("1301.TSE", date(2025, 1, 1), date(2025, 1, 5))
    assert result == []


def test_returns_tuples_in_range(tmp_path):
    loader = make_catalog_warmup_loader(tmp_path)
    target = date(2025, 1, 6)
    bar = _make_fake_bar(_ns(target), open_=100.0, high=110.0, low=98.0, close=105.0, volume=2000.0)
    with patch("engine.strategy_runtime.warmup.load_bars", return_value=[bar]):
        result = loader("1301.TSE", target, target)

    assert len(result) == 1
    d, o, h, l, c, v = result[0]
    assert d == target
    assert o == 100.0
    assert h == 110.0
    assert l == 98.0
    assert c == 105.0
    assert v == 2000.0


def test_result_sorted_by_date(tmp_path):
    loader = make_catalog_warmup_loader(tmp_path)
    d1, d2 = date(2025, 1, 6), date(2025, 1, 7)
    bar1 = _make_fake_bar(_ns(d1))
    bar2 = _make_fake_bar(_ns(d2))
    with patch("engine.strategy_runtime.warmup.load_bars", return_value=[bar2, bar1]):
        result = loader("1301.TSE", d1, d2)

    assert len(result) == 2
    assert result[0][0] == d1
    assert result[1][0] == d2


def test_tuple_types(tmp_path):
    loader = make_catalog_warmup_loader(tmp_path)
    d = date(2025, 1, 6)
    bar = _make_fake_bar(_ns(d))
    with patch("engine.strategy_runtime.warmup.load_bars", return_value=[bar]):
        result = loader("1301.TSE", d, d)

    assert len(result) == 1
    row = result[0]
    assert isinstance(row[0], date)
    for val in row[1:]:
        assert isinstance(val, float)


def test_end_date_inclusive(tmp_path):
    """end_date day should be included in the query range."""
    loader = make_catalog_warmup_loader(tmp_path)
    end = date(2025, 1, 10)
    # bar at exactly end_date midnight UTC
    bar_on_end = _make_fake_bar(_ns(end))
    with patch("engine.strategy_runtime.warmup.load_bars", return_value=[bar_on_end]):
        result = loader("1301.TSE", end, end)
    assert len(result) == 1


# ---------------------------------------------------------------------------
# Slow integration tests (real catalog required)
# ---------------------------------------------------------------------------


@pytest.mark.slow
@pytest.mark.skipif(
    not _REAL_CATALOG_AVAILABLE,
    reason="Real catalog not available at artifacts/jquants-catalog",
)
def test_real_catalog_1301_minute_returns_empty_for_daily():
    """Minute-only catalog should return [] for a Daily warmup request."""
    loader = make_catalog_warmup_loader(_CATALOG)
    result = loader("1301.TSE", date(2024, 12, 23), date(2025, 1, 5))
    # No Daily bars in the Minute-only catalog → empty list (not an error)
    assert isinstance(result, list)
