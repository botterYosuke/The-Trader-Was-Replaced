"""LiveEvent discriminated union spec (Phase 8 §3.2 / §3.3 reducer 接続点)。

Tachibana / kabu いずれの adapter も events() からこの 3 種のいずれかを
yield する。reducer は kind フィールドで分岐する。
"""

import pytest
from pydantic import ValidationError

from engine.live.adapter import (
    DepthLevel,
    DepthUpdate,
    KlineUpdate,
    LiveEvent,
    TradesUpdate,
)


def test_kline_update_minimum_fields():
    ev = KlineUpdate(
        kind="kline",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_000_000_000,
        open=2500.0,
        high=2510.0,
        low=2495.0,
        close=2505.0,
        volume=12345,
    )
    assert ev.kind == "kline"
    assert ev.instrument_id == "7203.TSE"


def test_trades_update_minimum_fields():
    ev = TradesUpdate(
        kind="trades",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_000_000_000,
        price=2505.0,
        size=100,
        aggressor_side="buy",
    )
    assert ev.aggressor_side == "buy"


def test_trades_update_rejects_invalid_side():
    with pytest.raises(ValidationError):
        TradesUpdate(
            kind="trades",
            instrument_id="7203.TSE",
            ts_ns=0,
            price=1.0,
            size=1,
            aggressor_side="??",  # type: ignore[arg-type]
        )


def test_depth_update_ten_levels():
    bids = [DepthLevel(price=2500.0 - i, size=100 * (i + 1)) for i in range(10)]
    asks = [DepthLevel(price=2501.0 + i, size=100 * (i + 1)) for i in range(10)]
    ev = DepthUpdate(
        kind="depth",
        instrument_id="7203.TSE",
        ts_ns=1_700_000_000_000_000_000,
        bids=bids,
        asks=asks,
    )
    assert len(ev.bids) == 10
    assert len(ev.asks) == 10


def test_live_event_union_dispatch():
    # 静的 union として import 可能なこと
    events: list[LiveEvent] = [
        KlineUpdate(
            kind="kline",
            instrument_id="7203.TSE",
            ts_ns=0,
            open=1, high=1, low=1, close=1, volume=0,
        ),
        TradesUpdate(
            kind="trades",
            instrument_id="7203.TSE",
            ts_ns=0,
            price=1, size=1, aggressor_side="sell",
        ),
        DepthUpdate(
            kind="depth",
            instrument_id="7203.TSE",
            ts_ns=0,
            bids=[], asks=[],
        ),
    ]
    kinds = [getattr(e, "kind") for e in events]
    assert kinds == ["kline", "trades", "depth"]
