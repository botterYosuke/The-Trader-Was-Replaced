"""Tests for instrument_id_to_bar_type helper (D17)."""

from __future__ import annotations

import pytest

from engine.core import instrument_id_to_bar_type


def test_minute_granularity_appends_minute_spec():
    result = instrument_id_to_bar_type("1301.TSE", "Minute")
    assert result == "1301.TSE-1-MINUTE-LAST-EXTERNAL"


def test_daily_granularity_appends_day_spec():
    result = instrument_id_to_bar_type("7203.TSE", "Daily")
    assert result == "7203.TSE-1-DAY-LAST-EXTERNAL"


def test_unknown_granularity_falls_back_to_minute():
    result = instrument_id_to_bar_type("AAPL.NASDAQ", "Trade")
    assert result == "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"
