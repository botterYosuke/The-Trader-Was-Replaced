"""Phase 10 Step 1 — Live Bar 供給の設計確定 (PoC + 回帰).

ADR-B (§2.3): Replay は catalog の EXTERNAL `Bar` を `BacktestEngine` 経由で
`on_bar` に流す。Live は venue tick を Nautilus `TradeTick` 化し、Nautilus 標準の
internal aggregation (`data/aggregation.pyx` の `TimeBarAggregator`) で INTERNAL
`Bar` を生成して同じ `on_bar` に届ける。

このテストが構造的にロックする設計判断:
  1. 戦略は同じ `BarSpecification`（step / aggregation / price_type）を購読し続け、
     変わるのは `aggregation_source`（EXTERNAL → INTERNAL）だけ。完全一致は要求しない。
  2. 同じ tick 列から Nautilus internal aggregation が作る `Bar` の OHLCV が、
     手計算（open=最初 / high=最大 / low=最小 / close=最後 / volume=合計）と一致する。
  3. `strategy_loader.load()` は環境非依存（クラスを返すだけでインスタンス化も
     clock/data 束縛もしない）→ Replay/Live の双方から呼べる。
"""

from __future__ import annotations

import importlib.util
import inspect
from pathlib import Path

import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.data.aggregation import TimeBarAggregator
from nautilus_trader.model.data import BarType, TradeTick
from nautilus_trader.model.enums import AggregationSource, AggressorSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price, Quantity

from engine.live.bar_supply import live_bar_type, to_internal_bar_type
from engine.strategy_runtime.catalog_data_loader import bar_type_for_instrument
from engine.strategy_runtime.instrument_factory import make_equity_instrument

_MINUTE_NS = 60_000_000_000
_FIXTURE_DIR = Path(__file__).parent.parent / "fixtures" / "strategies"


# ---------------------------------------------------------------------------
# to_internal_bar_type / live_bar_type
# ---------------------------------------------------------------------------


def test_to_internal_bar_type_swaps_external_suffix():
    assert (
        to_internal_bar_type("1301.TSE-1-MINUTE-LAST-EXTERNAL")
        == "1301.TSE-1-MINUTE-LAST-INTERNAL"
    )


def test_to_internal_bar_type_is_idempotent_on_internal():
    assert (
        to_internal_bar_type("1301.TSE-1-DAY-LAST-INTERNAL")
        == "1301.TSE-1-DAY-LAST-INTERNAL"
    )


def test_to_internal_bar_type_rejects_unsuffixed():
    with pytest.raises(ValueError, match="EXTERNAL or -INTERNAL"):
        to_internal_bar_type("1301.TSE-1-MINUTE-LAST")


def test_live_bar_type_matches_external_spec_but_internal_source():
    external = BarType.from_str(bar_type_for_instrument("1301.TSE", "Minute"))
    internal = BarType.from_str(live_bar_type("1301.TSE", "Minute"))

    # 戦略が購読する instrument / spec は不変
    assert internal.instrument_id == external.instrument_id
    assert internal.spec == external.spec
    # 変わるのは aggregation_source だけ
    assert external.aggregation_source == AggregationSource.EXTERNAL
    assert internal.aggregation_source == AggregationSource.INTERNAL


# ---------------------------------------------------------------------------
# Nautilus internal aggregation PoC — TradeTick → Bar
# ---------------------------------------------------------------------------


def test_internal_aggregation_produces_bar_matching_external_ohlcv():
    """同一 tick 列を Nautilus 標準 TimeBarAggregator(INTERNAL) に流し、
    OHLCV が手計算と一致し、spec が EXTERNAL catalog BarType と揃うことを確認する。"""
    instrument = make_equity_instrument("1301", "TSE")
    internal_bt = BarType.from_str(live_bar_type("1301.TSE", "Minute"))

    captured: list = []
    clock = TestClock()
    start_ns = 5 * _MINUTE_NS + 10_000_000  # 5:00.010 — 直近の分境界の少し後
    clock.set_time(start_ns)

    aggregator = TimeBarAggregator(instrument, internal_bt, captured.append, clock)
    aggregator.start_timer()

    def _tick(price: float, size: int, ts: int) -> TradeTick:
        return TradeTick(
            instrument_id=instrument.id,
            price=Price(price, precision=1),
            size=Quantity(size, precision=0),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId(f"T{ts}"),
            ts_event=ts,
            ts_init=ts,
        )

    # (5:00, 6:00] の確定バーに 4 tick を集約
    aggregator.handle_trade_tick(_tick(1000.0, 100, start_ns))            # open
    aggregator.handle_trade_tick(_tick(1010.0, 200, start_ns + 20 * 1_000_000_000))  # high
    aggregator.handle_trade_tick(_tick(990.0, 300, start_ns + 40 * 1_000_000_000))   # low
    aggregator.handle_trade_tick(_tick(1005.0, 400, 6 * _MINUTE_NS))      # close

    events = clock.advance_time(6 * _MINUTE_NS)
    assert len(events) == 1, f"expected one bar-close timer event, got {len(events)}"
    events[0].handle()

    assert len(captured) == 1, f"expected exactly one Bar, got {len(captured)}"
    bar = captured[0]

    assert bar.open == Price(1000.0, precision=1)
    assert bar.high == Price(1010.0, precision=1)
    assert bar.low == Price(990.0, precision=1)
    assert bar.close == Price(1005.0, precision=1)
    assert bar.volume == Quantity(1000, precision=0)

    # 同じ spec、source だけ INTERNAL
    external_bt = BarType.from_str(bar_type_for_instrument("1301.TSE", "Minute"))
    assert bar.bar_type.spec == external_bt.spec
    assert bar.bar_type.instrument_id == external_bt.instrument_id
    assert bar.bar_type.aggregation_source == AggregationSource.INTERNAL


# ---------------------------------------------------------------------------
# Strategy portability — loader は環境非依存
# ---------------------------------------------------------------------------


def _load_fixture(name: str):
    path = _FIXTURE_DIR / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)  # type: ignore[arg-type]
    spec.loader.exec_module(mod)  # type: ignore[union-attr]
    return mod


def test_loader_returns_class_without_instantiating():
    """strategy_loader.load() は strategy_cls（クラス）を返し、インスタンス化や
    clock/data 束縛をしない。これが Replay/Live 双方から同じロードを使える根拠。"""
    from engine.strategy_runtime.strategy_loader import load

    fixture = _FIXTURE_DIR / "fake_buy_and_hold.py"
    _module, _scenario, strategy_cls = load(fixture)

    from nautilus_trader.trading.strategy import Strategy

    # クラスそのものが返る（インスタンス化されていない）— clock/cache/msgbus は
    # register() 時にエンジンが注入するため、ロード時点では束縛されない（§0′-2）。
    assert inspect.isclass(strategy_cls)
    assert issubclass(strategy_cls, Strategy)


def test_same_strategy_config_accepts_internal_bar_type():
    """同じ戦略 config が EXTERNAL/INTERNAL いずれの bar_type も受け取れる
    （Live host が INTERNAL を供給して無分岐で動かす根拠、§0.4）。"""
    mod = _load_fixture("fake_buy_and_hold")
    cfg = mod.FakeBuyAndHoldConfig(
        instrument_id="1301.TSE",
        bar_type=to_internal_bar_type("1301.TSE-1-DAY-LAST-EXTERNAL"),
    )
    assert cfg.bar_type.endswith("-INTERNAL")
    # BarType として parse 可能（戦略 on_start が subscribe_bars で使う）
    assert BarType.from_str(cfg.bar_type).aggregation_source == AggregationSource.INTERNAL
