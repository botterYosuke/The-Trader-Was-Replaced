"""Tests for tachibana_ws (Phase 8 §3.2 A3.1: is_market_open + FdFrameProcessor)."""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal

import pytest

from engine.exchanges.tachibana_ws import FdFrameProcessor, is_market_open

JST = timezone(timedelta(hours=9))


# ---------------------------------------------------------------------------
# is_market_open
# ---------------------------------------------------------------------------


def test_is_market_open_morning_session():
    # 10:00 JST is inside 前場 (09:00–11:30).
    assert is_market_open(datetime(2026, 5, 18, 10, 0, tzinfo=JST)) is True


def test_is_market_open_lunch_break_closed():
    # 12:00 JST is inside 昼休 (11:30–12:30).
    assert is_market_open(datetime(2026, 5, 18, 12, 0, tzinfo=JST)) is False


def test_is_market_open_after_close():
    # 15:35 JST is past クロージング (15:30 end).
    assert is_market_open(datetime(2026, 5, 18, 15, 35, tzinfo=JST)) is False


def test_is_market_open_naive_datetime_treated_as_utc():
    # 00:30 UTC == 09:30 JST → 前場内.
    naive = datetime(2026, 5, 18, 0, 30)
    assert is_market_open(naive) is True


# ---------------------------------------------------------------------------
# FdFrameProcessor — first frame initializes, no trade emitted
# ---------------------------------------------------------------------------


def test_first_frame_initializes_state_returns_no_trade():
    p = FdFrameProcessor(row="1")
    trade, _depth = p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_000_000,
    )
    assert trade is None


# ---------------------------------------------------------------------------
# FdFrameProcessor — DV increase emits a trade with qty = delta
# ---------------------------------------------------------------------------


def test_dv_increase_emits_trade_with_delta_qty():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_000_000,
    )
    trade, _depth = p.process(
        {"p_1_DPP": "3001", "p_1_DV": "1500",
         "p_1_GBP1": "3000", "p_1_GAP1": "3002"},
        recv_ts_ms=1_700_000_001_000,
    )
    assert trade is not None
    assert trade["price"] == "3001"
    assert trade["qty"] == "500"
    # price 3001 >= prev_ask 3001 → buy.
    assert trade["side"] == "buy"
    assert trade["is_liquidation"] is False


# ---------------------------------------------------------------------------
# FdFrameProcessor — DV reset (session rollover) reinitializes without trade
# ---------------------------------------------------------------------------


def test_dv_reset_reinitializes_without_trade():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "5000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_000_000,
    )
    trade, _depth = p.process(
        {"p_1_DPP": "3000", "p_1_DV": "100",  # DV decreased → reset
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_001_000,
    )
    assert trade is None


# ---------------------------------------------------------------------------
# FdFrameProcessor — depth extraction (bid/ask ladders)
# ---------------------------------------------------------------------------


def test_depth_extracts_bid_ask_ladders():
    p = FdFrameProcessor(row="1")
    fields = {
        "p_1_DPP": "3000", "p_1_DV": "1000",
        "p_1_GBP1": "2999", "p_1_GBV1": "100",
        "p_1_GBP2": "2998", "p_1_GBV2": "200",
        "p_1_GAP1": "3001", "p_1_GAV1": "150",
        "p_1_GAP2": "3002", "p_1_GAV2": "250",
    }
    _trade, depth = p.process(fields, recv_ts_ms=1_700_000_000_000)
    assert depth is not None
    assert depth["bids"] == [
        {"price": "2999", "qty": "100"},
        {"price": "2998", "qty": "200"},
    ]
    assert depth["asks"] == [
        {"price": "3001", "qty": "150"},
        {"price": "3002", "qty": "250"},
    ]
    assert depth["sequence_id"] == 1
    assert depth["recv_ts_ms"] == 1_700_000_000_000


# ---------------------------------------------------------------------------
# FdFrameProcessor — side rules (quote rule + tick rule)
# ---------------------------------------------------------------------------


def test_side_at_or_above_ask_is_buy():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_000,
    )
    trade, _ = p.process(
        {"p_1_DPP": "3001", "p_1_DV": "1100",
         "p_1_GBP1": "3000", "p_1_GAP1": "3002"},
        recv_ts_ms=2_000,
    )
    assert trade is not None and trade["side"] == "buy"


def test_side_at_or_below_bid_is_sell():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_000,
    )
    trade, _ = p.process(
        {"p_1_DPP": "2999", "p_1_DV": "1100",
         "p_1_GBP1": "2998", "p_1_GAP1": "3000"},
        recv_ts_ms=2_000,
    )
    assert trade is not None and trade["side"] == "sell"


# ---------------------------------------------------------------------------
# FdFrameProcessor — reset() clears state so the next frame is treated as first
# ---------------------------------------------------------------------------


def test_reset_clears_prev_state():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_000,
    )
    p.reset()
    trade, _ = p.process(
        {"p_1_DPP": "3001", "p_1_DV": "9999",
         "p_1_GBP1": "3000", "p_1_GAP1": "3002"},
        recv_ts_ms=2_000,
    )
    # After reset, prev_dv is None again → first frame, no trade despite DV jump.
    assert trade is None
