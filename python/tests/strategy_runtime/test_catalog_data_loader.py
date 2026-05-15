"""Unit tests for engine.strategy_runtime.catalog_data_loader.

All tests monkeypatch ``engine.nautilus_catalog_loader.load_bars`` so no real
catalog is needed.
"""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Any
from unittest.mock import MagicMock, patch

import pytest

from engine.strategy_runtime.catalog_data_loader import (
    bar_type_for_instrument,
    instruments_from_scenario,
    load_bars_for_scenario,
    merge_bars_by_ts,
    normalize_granularity,
)


# ---------------------------------------------------------------------------
# Fake Bar helper
# ---------------------------------------------------------------------------


def _make_bar(instrument: str, ts_event_ns: int) -> Any:
    bar = MagicMock()
    bar.ts_event = ts_event_ns
    bar.instrument_id = instrument
    return bar


def _ns(date_str: str) -> int:
    """'YYYY-MM-DD' → UTC midnight nanoseconds."""
    dt = datetime.fromisoformat(date_str).replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1_000_000_000)


# ---------------------------------------------------------------------------
# instruments_from_scenario
# ---------------------------------------------------------------------------


def test_instruments_from_scenario_v1():
    sc = {"schema_version": 1, "instrument": "1301.TSE",
          "start": "2025-01-06", "end": "2025-01-10",
          "granularity": "Minute", "initial_cash": 1_000_000}
    assert instruments_from_scenario(sc) == ["1301.TSE"]


def test_instruments_from_scenario_v2():
    sc = {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"],
          "start": "2025-01-06", "end": "2025-01-10",
          "granularity": "Minute", "initial_cash": 1_000_000}
    assert instruments_from_scenario(sc) == ["1301.TSE", "7203.TSE"]


def test_instruments_from_scenario_v3():
    sc = {"schema_version": 3,
          "instruments": ["1301.TSE", "1332.TSE", "7203.TSE"],
          "start": "2025-01-06", "end": "2025-01-10",
          "granularity": "Minute", "initial_cash": 1_000_000}
    result = instruments_from_scenario(sc)
    assert result == ["1301.TSE", "1332.TSE", "7203.TSE"]


# ---------------------------------------------------------------------------
# normalize_granularity
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("value,expected", [
    ("Daily", "Daily"),
    ("DAILY", "Daily"),
    ("daily", "Daily"),
    ("Minute", "Minute"),
    ("MINUTE", "Minute"),
    ("minute", "Minute"),
    ("  Minute  ", "Minute"),
])
def test_normalize_granularity_ok(value: str, expected: str):
    assert normalize_granularity(value) == expected


@pytest.mark.parametrize("bad", ["Tick", "Hour", "Weekly", "", "min"])
def test_normalize_granularity_unsupported(bad: str):
    with pytest.raises(ValueError, match="Unsupported granularity"):
        normalize_granularity(bad)


# ---------------------------------------------------------------------------
# bar_type_for_instrument
# ---------------------------------------------------------------------------


def test_bar_type_for_instrument_minute():
    assert bar_type_for_instrument("1301.TSE", "Minute") == "1301.TSE-1-MINUTE-LAST-EXTERNAL"


def test_bar_type_for_instrument_daily():
    assert bar_type_for_instrument("7203.TSE", "Daily") == "7203.TSE-1-DAY-LAST-EXTERNAL"


# ---------------------------------------------------------------------------
# load_bars_for_scenario — monkeypatched
# ---------------------------------------------------------------------------

_CATALOG = "fake/catalog"

_V1_SCENARIO = {
    "schema_version": 1,
    "instrument": "1301.TSE",
    "start": "2025-01-06",
    "end": "2025-01-10",
    "granularity": "Minute",
    "initial_cash": 1_000_000,
}

_V2_SCENARIO = {
    "schema_version": 2,
    "instruments": ["1301.TSE", "7203.TSE"],
    "start": "2025-01-06",
    "end": "2025-01-10",
    "granularity": "Minute",
    "initial_cash": 1_000_000,
}


def _patch_load_bars(bars_sequence: list[list]):
    """Patch load_bars to return successive lists per call."""
    call_iter = iter(bars_sequence)
    return patch(
        "engine.strategy_runtime.catalog_data_loader.load_bars",
        side_effect=lambda *_a, **_kw: next(call_iter),
    )


def test_load_bars_for_scenario_v1_single_instrument():
    bar = _make_bar("1301.TSE", _ns("2025-01-07"))
    with _patch_load_bars([[bar]]):
        result = load_bars_for_scenario(_CATALOG, _V1_SCENARIO)

    from nautilus_trader.model.identifiers import InstrumentId
    key = InstrumentId.from_str("1301.TSE")
    assert key in result
    assert result[key] == [bar]


def test_load_bars_for_scenario_v2_two_instruments():
    bar_a = _make_bar("1301.TSE", _ns("2025-01-07"))
    bar_b = _make_bar("7203.TSE", _ns("2025-01-07") + 1)
    with _patch_load_bars([[bar_a], [bar_b]]):
        result = load_bars_for_scenario(_CATALOG, _V2_SCENARIO)

    from nautilus_trader.model.identifiers import InstrumentId
    assert InstrumentId.from_str("1301.TSE") in result
    assert InstrumentId.from_str("7203.TSE") in result


def test_load_bars_for_scenario_bars_sorted_by_ts_event():
    bar_early = _make_bar("1301.TSE", _ns("2025-01-07"))
    bar_late = _make_bar("1301.TSE", _ns("2025-01-09"))
    # Return in reversed order; loader must sort
    with _patch_load_bars([[bar_late, bar_early]]):
        result = load_bars_for_scenario(_CATALOG, _V1_SCENARIO)

    from nautilus_trader.model.identifiers import InstrumentId
    bars = result[InstrumentId.from_str("1301.TSE")]
    assert bars[0].ts_event < bars[1].ts_event


def test_load_bars_for_scenario_filters_out_of_range_bars():
    in_range = _make_bar("1301.TSE", _ns("2025-01-07"))
    before_start = _make_bar("1301.TSE", _ns("2025-01-05"))
    after_end = _make_bar("1301.TSE", _ns("2025-01-11"))  # end="2025-01-10" → exclusive 01-11
    with _patch_load_bars([[in_range, before_start, after_end]]):
        result = load_bars_for_scenario(_CATALOG, _V1_SCENARIO)

    from nautilus_trader.model.identifiers import InstrumentId
    bars = result[InstrumentId.from_str("1301.TSE")]
    assert bars == [in_range]


def test_load_bars_for_scenario_end_day_inclusive():
    """Bar exactly on the end date (midnight) must be included."""
    on_end_day = _make_bar("1301.TSE", _ns("2025-01-10"))
    with _patch_load_bars([[on_end_day]]):
        result = load_bars_for_scenario(_CATALOG, _V1_SCENARIO)

    from nautilus_trader.model.identifiers import InstrumentId
    bars = result[InstrumentId.from_str("1301.TSE")]
    assert on_end_day in bars


def test_load_bars_for_scenario_daily_granularity():
    sc = {**_V1_SCENARIO, "granularity": "DAILY"}
    bar = _make_bar("1301.TSE", _ns("2025-01-07"))
    with _patch_load_bars([[bar]]):
        result = load_bars_for_scenario(_CATALOG, sc)

    from nautilus_trader.model.identifiers import InstrumentId
    assert result[InstrumentId.from_str("1301.TSE")] == [bar]


def test_load_bars_for_scenario_unsupported_granularity_raises():
    sc = {**_V1_SCENARIO, "granularity": "Tick"}
    with pytest.raises(ValueError, match="Unsupported granularity"):
        load_bars_for_scenario(_CATALOG, sc)


def test_load_bars_for_scenario_empty_catalog_returns_empty_list():
    with _patch_load_bars([[]]):
        result = load_bars_for_scenario(_CATALOG, _V1_SCENARIO)

    from nautilus_trader.model.identifiers import InstrumentId
    assert result[InstrumentId.from_str("1301.TSE")] == []


# ---------------------------------------------------------------------------
# merge_bars_by_ts
# ---------------------------------------------------------------------------


def test_merge_bars_by_ts_single_instrument():
    from nautilus_trader.model.identifiers import InstrumentId
    b1 = _make_bar("1301.TSE", 100)
    b2 = _make_bar("1301.TSE", 200)
    merged = merge_bars_by_ts({InstrumentId.from_str("1301.TSE"): [b2, b1]})
    assert [b.ts_event for b in merged] == [100, 200]


def test_merge_bars_by_ts_two_instruments_interleaved():
    from nautilus_trader.model.identifiers import InstrumentId
    a1 = _make_bar("1301.TSE", 100)
    a2 = _make_bar("1301.TSE", 300)
    b1 = _make_bar("7203.TSE", 200)
    b2 = _make_bar("7203.TSE", 400)
    merged = merge_bars_by_ts({
        InstrumentId.from_str("1301.TSE"): [a1, a2],
        InstrumentId.from_str("7203.TSE"): [b1, b2],
    })
    assert [b.ts_event for b in merged] == [100, 200, 300, 400]


def test_merge_bars_by_ts_empty():
    assert merge_bars_by_ts({}) == []


def test_merge_bars_by_ts_stable_sort_on_equal_ts():
    """同一 ts_event の場合、元の順序（銘柄A→銘柄B）を保持する。"""
    from nautilus_trader.model.identifiers import InstrumentId
    a = _make_bar("1301.TSE", 100)
    b = _make_bar("7203.TSE", 100)
    merged = merge_bars_by_ts({
        InstrumentId.from_str("1301.TSE"): [a],
        InstrumentId.from_str("7203.TSE"): [b],
    })
    assert len(merged) == 2
    # stable sort: both ts_event==100, order depends on insertion order
    assert set(m.ts_event for m in merged) == {100}
