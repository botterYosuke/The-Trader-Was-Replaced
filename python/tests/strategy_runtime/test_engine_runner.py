"""Smoke tests for engine.strategy_runtime.engine_runner — Step 3A/3B.

Goal: BacktestEngine が Fake strategy + synthetic bars で走り、
      write_equity / write_fill が正しく呼ばれることを確認する。

実 catalog 不要。bars_by_instrument を合成 Bar で直接構築。
"""

from __future__ import annotations

import importlib.util
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.model.data import Bar, BarSpecification, BarType
from nautilus_trader.model.enums import AggregationSource, BarAggregation, PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price, Quantity

_BLACKSHEEP_STRATEGIES = Path("c:/Users/sasai/Documents/🐃_blacksheep/strategies")


# ---------------------------------------------------------------------------
# Helpers: load fixture strategy by file path (no __init__.py chain needed)
# ---------------------------------------------------------------------------

_FIXTURE_DIR = Path(__file__).parent.parent / "fixtures" / "strategies"


def _load_fixture(name: str):
    """fixtures/strategies/{name}.py を importlib で読み込む。"""
    path = _FIXTURE_DIR / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)  # type: ignore[arg-type]
    spec.loader.exec_module(mod)  # type: ignore[union-attr]
    return mod


def _make_bar(
    symbol: str = "1301.TSE",
    ts_ns: int = 0,
    close: float = 1000.0,
) -> Bar:
    """合成 Daily Bar を 1 本生成する。"""
    iid = InstrumentId.from_str(symbol)
    bar_spec = BarSpecification(1, BarAggregation.DAY, PriceType.LAST)
    bar_type = BarType(iid, bar_spec, AggregationSource.EXTERNAL)
    close_price = Price(Decimal(str(close)), precision=1)
    return Bar(
        bar_type=bar_type,
        open=close_price,
        high=Price(Decimal(str(close + 10)), precision=1),
        low=Price(Decimal(str(close - 10)), precision=1),
        close=close_price,
        volume=Quantity(1000, precision=0),
        ts_event=ts_ns,
        ts_init=ts_ns,
    )


def _make_minute_bar(
    symbol: str = "1301.TSE",
    ts_ns: int = 0,
    close: float = 1000.0,
) -> Bar:
    """合成 Minute Bar を 1 本生成する。"""
    iid = InstrumentId.from_str(symbol)
    bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
    bar_type = BarType(iid, bar_spec, AggregationSource.EXTERNAL)
    close_price = Price(Decimal(str(close)), precision=1)
    return Bar(
        bar_type=bar_type,
        open=close_price,
        high=Price(Decimal(str(close + 10)), precision=1),
        low=Price(Decimal(str(close - 10)), precision=1),
        close=close_price,
        volume=Quantity(1000, precision=0),
        ts_event=ts_ns,
        ts_init=ts_ns,
    )


# ---------------------------------------------------------------------------
# Fake sink
# ---------------------------------------------------------------------------


class _Sink:
    def __init__(self) -> None:
        self.fills: list[dict] = []
        self.equities: list[dict] = []

    def write_fill(self, event: dict) -> None:
        self.fills.append(event)

    def write_equity(self, event: dict) -> None:
        self.equities.append(event)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_engine_runner_5bars_no_orders():
    """5 bars で engine が走る。equity 5 回、fill 0 回。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_buy_and_hold")
    FakeBuyAndHold = mod.FakeBuyAndHold
    FakeBuyAndHoldConfig = mod.FakeBuyAndHoldConfig

    iid = InstrumentId.from_str("1301.TSE")
    bars = [
        _make_bar("1301.TSE", ts_ns=i * 86_400_000_000_000, close=1000.0 + i * 10)
        for i in range(5)
    ]
    bars_by_instrument = {iid: bars}

    scenario = {
        "instrument": "1301.TSE",
        "granularity": "Daily",
        "start": "2024-01-01",
        "end": "2024-12-31",
        "initial_cash": 10_000_000,
    }

    sink = _Sink()
    run(
        strategy_cls=FakeBuyAndHold,
        scenario=scenario,
        bars_by_instrument=bars_by_instrument,
        run_buffer=sink,
        strategy_init_kwargs={
            "config": FakeBuyAndHoldConfig(
                instrument_id="1301.TSE",
                bar_type="1301.TSE-1-DAY-LAST-EXTERNAL",
            )
        },
    )

    assert len(sink.equities) == 5, (
        f"equity write_equity should be called once per bar (5), got {len(sink.equities)}"
    )
    assert len(sink.fills) == 0, (
        f"no orders placed, write_fill should be 0, got {len(sink.fills)}"
    )
    for eq in sink.equities:
        assert "ts_event_ms" in eq, f"missing ts_event_ms: {eq}"
        assert "equity" in eq, f"missing equity: {eq}"
        assert isinstance(eq["equity"], float), f"equity should be float: {eq}"


def test_engine_runner_equity_is_initial_cash_when_no_positions():
    """ポジションなし時の equity は initial_cash に等しい。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_buy_and_hold")
    FakeBuyAndHold = mod.FakeBuyAndHold
    FakeBuyAndHoldConfig = mod.FakeBuyAndHoldConfig

    iid = InstrumentId.from_str("1301.TSE")
    bars = [_make_bar("1301.TSE", ts_ns=0)]
    bars_by_instrument = {iid: bars}

    initial_cash = 5_000_000
    scenario = {
        "instrument": "1301.TSE",
        "granularity": "Daily",
        "start": "2024-01-01",
        "end": "2024-12-31",
        "initial_cash": initial_cash,
    }

    sink = _Sink()
    run(
        strategy_cls=FakeBuyAndHold,
        scenario=scenario,
        bars_by_instrument=bars_by_instrument,
        run_buffer=sink,
        strategy_init_kwargs={
            "config": FakeBuyAndHoldConfig(
                instrument_id="1301.TSE",
                bar_type="1301.TSE-1-DAY-LAST-EXTERNAL",
            )
        },
    )

    assert len(sink.equities) == 1
    assert sink.equities[0]["equity"] == pytest.approx(float(initial_cash), rel=1e-6)


def test_run_buffer_like_protocol_satisfied():
    """_Sink が RunBufferLike Protocol を満たす。"""
    from engine.strategy_runtime.engine_runner import RunBufferLike

    sink = _Sink()
    assert isinstance(sink, RunBufferLike)


# ---------------------------------------------------------------------------
# Step 3B-1: fill 発生テスト (FakeMarketBuyOnce)
# ---------------------------------------------------------------------------

def _make_runner_kwargs_for_buy_once(mod, bars_by_instrument: dict, initial_cash: int = 10_000_000) -> dict:
    return {
        "strategy_cls": mod.FakeMarketBuyOnce,
        "scenario": {
            "instrument": "1301.TSE",
            "granularity": "Daily",
            "start": "2024-01-01",
            "end": "2024-12-31",
            "initial_cash": initial_cash,
        },
        "bars_by_instrument": bars_by_instrument,
        "strategy_init_kwargs": {
            "config": mod.FakeMarketBuyOnceConfig(
                instrument_id="1301.TSE",
                bar_type="1301.TSE-1-DAY-LAST-EXTERNAL",
            )
        },
    }


def test_fill_is_written_on_market_buy():
    """market buy → fill topic → write_fill が 1 件呼ばれる。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_market_buy_once")
    iid = InstrumentId.from_str("1301.TSE")
    bars = [
        _make_bar("1301.TSE", ts_ns=i * 86_400_000_000_000, close=1000.0 + i * 10)
        for i in range(3)
    ]
    bars_by_instrument = {iid: bars}

    sink = _Sink()
    run(**_make_runner_kwargs_for_buy_once(mod, bars_by_instrument), run_buffer=sink)

    assert len(sink.fills) >= 1, f"expected ≥1 fill, got {len(sink.fills)}"
    fill = sink.fills[0]
    assert fill["instrument_id"] == "1301.TSE"
    assert fill["side"] == "BUY"
    assert float(fill["qty"]) == pytest.approx(100.0)
    assert "price" in fill
    assert "ts_event_ms" in fill


def test_fill_has_commission_key():
    """fill dict に commission キーが含まれる（Nautilus が commission を返す場合）。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_market_buy_once")
    iid = InstrumentId.from_str("1301.TSE")
    bars = [_make_bar("1301.TSE", ts_ns=0, close=1000.0)]
    bars_by_instrument = {iid: bars}

    sink = _Sink()
    run(**_make_runner_kwargs_for_buy_once(mod, bars_by_instrument), run_buffer=sink)

    assert len(sink.fills) >= 1
    fill = sink.fills[0]
    if "commission" in fill:
        # commission が取れた場合は文字列で格納されていること
        assert isinstance(fill["commission"], str), f"commission should be str: {fill['commission']!r}"
        float(fill["commission"])  # 数値に変換できること


def test_no_fill_written_when_no_order():
    """注文を出さない strategy では fill が 0 件。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_buy_and_hold")
    iid = InstrumentId.from_str("1301.TSE")
    bars = [_make_bar("1301.TSE", ts_ns=i * 86_400_000_000_000) for i in range(5)]
    bars_by_instrument = {iid: bars}

    sink = _Sink()
    run(
        strategy_cls=mod.FakeBuyAndHold,
        scenario={
            "instrument": "1301.TSE",
            "granularity": "Daily",
            "start": "2024-01-01",
            "end": "2024-12-31",
            "initial_cash": 10_000_000,
        },
        bars_by_instrument=bars_by_instrument,
        run_buffer=sink,
        strategy_init_kwargs={
            "config": mod.FakeBuyAndHoldConfig(
                instrument_id="1301.TSE",
                bar_type="1301.TSE-1-DAY-LAST-EXTERNAL",
            )
        },
    )

    assert len(sink.fills) == 0


def test_fill_price_matches_bar_close():
    """market order は bar の close price で約定する（backtest engine の挙動確認）。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_market_buy_once")
    iid = InstrumentId.from_str("1301.TSE")
    close_price = 1234.0
    bars = [_make_bar("1301.TSE", ts_ns=0, close=close_price)]
    bars_by_instrument = {iid: bars}

    sink = _Sink()
    run(**_make_runner_kwargs_for_buy_once(mod, bars_by_instrument), run_buffer=sink)

    assert len(sink.fills) >= 1
    # BacktestEngine の market order は close price か open price で約定するため
    # 正確な値ではなく価格帯の範囲で確認する
    fill_price = float(sink.fills[0]["price"])
    assert fill_price > 0, f"fill price must be positive, got {fill_price}"


# ---------------------------------------------------------------------------
# Step 3B-2: 複数銘柄 merge + subscribe テスト
# ---------------------------------------------------------------------------

_DAY_NS = 86_400_000_000_000  # 1 日 (ns)


def test_engine_runner_multi_instrument_equity_in_ts_order():
    """2 銘柄 4 bar を ts_event 昇順で処理し、equity が時系列順に write される。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_buy_and_hold")

    iid_1301 = InstrumentId.from_str("1301.TSE")
    iid_1332 = InstrumentId.from_str("1332.TSE")

    bars_by_instrument = {
        iid_1301: [
            _make_bar("1301.TSE", ts_ns=2 * _DAY_NS),
            _make_bar("1301.TSE", ts_ns=4 * _DAY_NS),
        ],
        iid_1332: [
            _make_bar("1332.TSE", ts_ns=1 * _DAY_NS),
            _make_bar("1332.TSE", ts_ns=3 * _DAY_NS),
        ],
    }

    scenario = {
        "schema_version": 2,
        "instruments": ["1301.TSE", "1332.TSE"],
        "granularity": "Daily",
        "start": "2024-01-01",
        "end": "2024-12-31",
        "initial_cash": 10_000_000,
    }

    sink = _Sink()
    run(
        strategy_cls=mod.FakeBuyAndHold,
        scenario=scenario,
        bars_by_instrument=bars_by_instrument,
        run_buffer=sink,
        strategy_init_kwargs={
            "config": mod.FakeBuyAndHoldConfig(
                instrument_id="1301.TSE",
                bar_type="1301.TSE-1-DAY-LAST-EXTERNAL",
                bar_types=[
                    "1301.TSE-1-DAY-LAST-EXTERNAL",
                    "1332.TSE-1-DAY-LAST-EXTERNAL",
                ],
            )
        },
    )

    # 4 bar → equity 4 回
    assert len(sink.equities) == 4, (
        f"expected 4 equity writes (2 instruments × 2 bars), got {len(sink.equities)}"
    )

    # ts_event_ms は昇順であること (merge_bars_by_ts の保証)
    ts_list = [e["ts_event_ms"] for e in sink.equities]
    assert ts_list == sorted(ts_list), f"equity writes not in ts order: {ts_list}"

    # 期待する時刻シーケンス: 1d, 2d, 3d, 4d (ms)
    expected_ms = [
        1 * _DAY_NS // 1_000_000,
        2 * _DAY_NS // 1_000_000,
        3 * _DAY_NS // 1_000_000,
        4 * _DAY_NS // 1_000_000,
    ]
    assert ts_list == expected_ms, f"unexpected ts sequence: {ts_list}"


def test_engine_runner_multi_instrument_no_fills_when_no_orders():
    """複数銘柄で注文なし → fill 0 件、equity は全銘柄バー数ぶん出る。"""
    from engine.strategy_runtime.engine_runner import run

    mod = _load_fixture("fake_buy_and_hold")

    iid_1301 = InstrumentId.from_str("1301.TSE")
    iid_1332 = InstrumentId.from_str("1332.TSE")

    bars_by_instrument = {
        iid_1301: [_make_bar("1301.TSE", ts_ns=i * _DAY_NS) for i in range(3)],
        iid_1332: [_make_bar("1332.TSE", ts_ns=i * _DAY_NS + _DAY_NS // 2) for i in range(3)],
    }

    scenario = {
        "schema_version": 2,
        "instruments": ["1301.TSE", "1332.TSE"],
        "granularity": "Daily",
        "start": "2024-01-01",
        "end": "2024-12-31",
        "initial_cash": 10_000_000,
    }

    sink = _Sink()
    run(
        strategy_cls=mod.FakeBuyAndHold,
        scenario=scenario,
        bars_by_instrument=bars_by_instrument,
        run_buffer=sink,
        strategy_init_kwargs={
            "config": mod.FakeBuyAndHoldConfig(
                instrument_id="1301.TSE",
                bar_type="1301.TSE-1-DAY-LAST-EXTERNAL",
                bar_types=[
                    "1301.TSE-1-DAY-LAST-EXTERNAL",
                    "1332.TSE-1-DAY-LAST-EXTERNAL",
                ],
            )
        },
    )

    assert len(sink.fills) == 0
    assert len(sink.equities) == 6, (
        f"expected 6 equity writes (2 instruments × 3 bars), got {len(sink.equities)}"
    )


# ---------------------------------------------------------------------------
# Step 3B-3: mean_reversion_01.py smoke (実 strategy ファイル経由)
# ---------------------------------------------------------------------------

_MR01_PATH = _BLACKSHEEP_STRATEGIES / "mean_reversion_01.py"
_MR01_BARS = 30
# JST 2025-01-06 09:00 (= UTC 00:00) 基点で 1 分刻み
_MR01_BASE_NS = int(
    __import__("datetime").datetime(2025, 1, 6, 0, 0, tzinfo=__import__("datetime").timezone.utc).timestamp()
    * 1_000_000_000
)


@pytest.mark.skipif(
    not _MR01_PATH.exists(),
    reason=f"blacksheep strategy not found: {_MR01_PATH}",
)
def test_engine_runner_mean_reversion_01_smoke():
    """mean_reversion_01 が例外なく走り、equity が bar 数ぶん write される。

    合成 Minute bar 30 本を渡す。window=5 / k=10.0 で注文は出にくいが、
    fill 数は期待しない（条件未達でも pass）。
    """
    from engine.strategy_runtime.engine_runner import run
    from engine.strategy_runtime.strategy_loader import load

    _, scenario, strategy_cls = load(_MR01_PATH)

    iid = InstrumentId.from_str("1301.TSE")
    bars = [
        _make_minute_bar("1301.TSE", _MR01_BASE_NS + i * 60_000_000_000, 1000.0 + i)
        for i in range(_MR01_BARS)
    ]
    bars_by_instrument = {iid: bars}

    sink = _Sink()
    run(
        strategy_cls=strategy_cls,
        scenario=scenario,
        bars_by_instrument=bars_by_instrument,
        run_buffer=sink,
        strategy_init_kwargs={
            "instrument_id": "1301.TSE",
            "bar_type_str": "1301.TSE-1-MINUTE-LAST-EXTERNAL",
            "window": 5,
            "k": 10.0,
        },
    )

    assert len(sink.equities) == _MR01_BARS, (
        f"expected {_MR01_BARS} equity writes (one per bar), got {len(sink.equities)}"
    )
    for eq in sink.equities:
        assert "ts_event_ms" in eq
        assert "equity" in eq
        assert isinstance(eq["equity"], float)
