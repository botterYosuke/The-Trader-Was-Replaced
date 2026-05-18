"""TickBarAggregator spec (Phase 8 §3.7 / §ADR Tick→Bar 集約)。

責務: TradesUpdate (tick) を 1m bar (KlineUpdate) に集約する。
- on_tick(tick) -> Optional[KlineUpdate]:
    分境界を跨いだ瞬間に「直前の分の確定 bar」を返す。
    同一分内なら None。
- build_now() -> Optional[KlineUpdate]:
    現在進行中の partial bar をその時点の OHLCV で返す。
    まだ 1 tick も来ていなければ None。
- Replay と同形式の KlineUpdate を返す (kind="kline")。

Step B-2 スコープ:
- 時間枠は 1m 固定（multi-timeframe は Nautilus BarBuilder 統合時に拡張）。
- KlineUpdate / DepthUpdate は aggregator に渡さない設計（live_runner で振り分け）。
"""
from __future__ import annotations

import pytest

from engine.live.adapter import KlineUpdate, TradesUpdate
from engine.live.aggregator import TickBarAggregator


NS_PER_SEC = 1_000_000_000
NS_PER_MIN = 60 * NS_PER_SEC


def _tick(ts_ns: int, price: float, size: float = 1.0) -> TradesUpdate:
    return TradesUpdate(
        kind="trades",
        instrument_id="7203.TSE",
        ts_ns=ts_ns,
        price=price,
        size=size,
        aggressor_side="buy",
    )


def test_within_same_minute_returns_none():
    """同一分内の連続 tick では bar close は発火しない。"""
    agg = TickBarAggregator(instrument_id="7203.TSE", interval_ns=NS_PER_MIN)
    assert agg.on_tick(_tick(0 * NS_PER_MIN + 1, 100.0)) is None
    assert agg.on_tick(_tick(0 * NS_PER_MIN + 2, 101.0)) is None
    assert agg.on_tick(_tick(0 * NS_PER_MIN + 3, 99.0)) is None


def test_minute_rollover_emits_closed_bar_with_correct_ohlcv():
    """分境界を跨ぐ tick が来た瞬間に、直前分の確定 bar が返る。"""
    agg = TickBarAggregator(instrument_id="7203.TSE", interval_ns=NS_PER_MIN)
    agg.on_tick(_tick(0 * NS_PER_MIN + 1, 100.0, size=1.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 2, 105.0, size=2.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 3, 99.0, size=3.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 59 * NS_PER_SEC, 102.0, size=0.0))

    bar = agg.on_tick(_tick(1 * NS_PER_MIN + 1, 200.0, size=10.0))
    assert bar is not None
    assert isinstance(bar, KlineUpdate)
    assert bar.kind == "kline"
    assert bar.instrument_id == "7203.TSE"
    assert bar.ts_ns == 0
    assert bar.open == 100.0
    assert bar.high == 105.0
    assert bar.low == 99.0
    assert bar.close == 102.0
    assert bar.volume == 6.0


def test_build_now_returns_partial_bar_in_progress():
    """進行中 bar の OHLCV を、close を待たずに取得できる。"""
    agg = TickBarAggregator(instrument_id="7203.TSE", interval_ns=NS_PER_MIN)
    agg.on_tick(_tick(0 * NS_PER_MIN + 1, 100.0, size=1.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 2, 110.0, size=2.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 3, 95.0, size=3.0))

    partial = agg.build_now()
    assert partial is not None
    assert partial.kind == "kline"
    assert partial.ts_ns == 0
    assert partial.open == 100.0
    assert partial.high == 110.0
    assert partial.low == 95.0
    assert partial.close == 95.0
    assert partial.volume == 6.0


def test_build_now_before_any_tick_returns_none():
    agg = TickBarAggregator(instrument_id="7203.TSE", interval_ns=NS_PER_MIN)
    assert agg.build_now() is None


def test_skipping_multiple_minutes_emits_only_last_closed_bar():
    """tick が複数分跳ぶ場合、確定するのは直前 1 本だけ（空 bar は埋めない）。"""
    agg = TickBarAggregator(instrument_id="7203.TSE", interval_ns=NS_PER_MIN)
    agg.on_tick(_tick(0 * NS_PER_MIN + 1, 100.0, size=1.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 30 * NS_PER_SEC, 102.0, size=1.0))

    bar = agg.on_tick(_tick(3 * NS_PER_MIN + 1, 200.0, size=5.0))
    assert bar is not None
    assert bar.ts_ns == 0
    assert bar.open == 100.0
    assert bar.close == 102.0
    assert bar.volume == 2.0


def test_out_of_order_tick_within_current_bar_is_accepted():
    """同一分内なら ts_ns が前後しても OHLCV に反映される。"""
    agg = TickBarAggregator(instrument_id="7203.TSE", interval_ns=NS_PER_MIN)
    agg.on_tick(_tick(0 * NS_PER_MIN + 10 * NS_PER_SEC, 100.0, size=1.0))
    agg.on_tick(_tick(0 * NS_PER_MIN + 5 * NS_PER_SEC, 105.0, size=1.0))
    partial = agg.build_now()
    assert partial is not None
    assert partial.high == 105.0
    assert partial.volume == 2.0
