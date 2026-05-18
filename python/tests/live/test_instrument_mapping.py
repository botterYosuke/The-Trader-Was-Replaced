"""Tests for venue-agnostic InstrumentId mapping."""

from __future__ import annotations

import pytest

from engine.live.instrument_mapping import (
    instrument_id_to_kabu,
    kabu_to_instrument_id,
    tachibana_market_to_suffix,
)


def test_kabu_roundtrip_tse() -> None:
    assert kabu_to_instrument_id("5401", 1) == "5401.TSE"
    assert instrument_id_to_kabu("5401.TSE") == ("5401", 1)


def test_kabu_roundtrip_nse() -> None:
    assert kabu_to_instrument_id("9432", 3) == "9432.NSE"
    assert instrument_id_to_kabu("9432.NSE") == ("9432", 3)


def test_kabu_roundtrip_fse() -> None:
    assert kabu_to_instrument_id("1234", 5) == "1234.FSE"
    assert instrument_id_to_kabu("1234.FSE") == ("1234", 5)


def test_kabu_roundtrip_sse() -> None:
    assert kabu_to_instrument_id("5678", 6) == "5678.SSE"
    assert instrument_id_to_kabu("5678.SSE") == ("5678", 6)


def test_unknown_kabu_exchange_rejected() -> None:
    with pytest.raises(ValueError, match="UNKNOWN_VENUE_MARKET"):
        kabu_to_instrument_id("5401", 2)


def test_unknown_suffix_rejected() -> None:
    with pytest.raises(ValueError, match="UNKNOWN_VENUE_MARKET"):
        instrument_id_to_kabu("5401.XYZ")


def test_missing_suffix_rejected() -> None:
    with pytest.raises(ValueError, match="INVALID_INSTRUMENT_ID"):
        instrument_id_to_kabu("5401")


def test_empty_symbol_rejected() -> None:
    with pytest.raises(ValueError, match="INVALID_INSTRUMENT_ID"):
        instrument_id_to_kabu(".TSE")


def test_kabu_empty_symbol_rejected() -> None:
    with pytest.raises(ValueError, match="INVALID_SYMBOL"):
        kabu_to_instrument_id("", 1)


def test_tachibana_market_stub_raises() -> None:
    with pytest.raises(NotImplementedError):
        tachibana_market_to_suffix("東証")
