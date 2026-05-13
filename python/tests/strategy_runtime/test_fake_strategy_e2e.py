"""Step 4.5: engine_runner + RunBuffer 結合 E2E テスト。

synthetic bars で replay → run-buffer 3 ファイル生成が通ることを固定する。
実 catalog / 実 blacksheep 不要。
"""
from __future__ import annotations

from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.model.data import Bar, BarSpecification, BarType
from nautilus_trader.model.enums import AggregationSource, BarAggregation, PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price, Quantity

_FIXTURE_DIR = Path(__file__).parent.parent / "fixtures" / "strategies"
_FAKE_STRATEGY_PATH = _FIXTURE_DIR / "fake_market_buy_once.py"

_DAY_NS = 86_400_000_000_000


def _make_bar(
    symbol: str = "1301.TSE",
    ts_ns: int = 0,
    close: float = 1000.0,
) -> Bar:
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


@pytest.fixture()
def loaded_strategy():
    from engine.strategy_runtime.strategy_loader import load
    module, scenario, strategy_cls = load(_FAKE_STRATEGY_PATH)
    return module, scenario, strategy_cls


@pytest.fixture()
def synthetic_bars():
    iid = InstrumentId.from_str("1301.TSE")
    bars = [
        _make_bar("1301.TSE", ts_ns=i * _DAY_NS, close=1000.0 + i * 10)
        for i in range(5)
    ]
    return {iid: bars}


class TestRunBufferFilesProduced:
    def test_meta_json_exists(self, tmp_path, loaded_strategy, synthetic_bars):
        from engine.strategy_runtime.engine_runner import run
        from engine.strategy_runtime.run_buffer import RunBuffer, make_run_id

        _, scenario, strategy_cls = loaded_strategy
        run_id = make_run_id(str(_FAKE_STRATEGY_PATH), "1301.TSE")
        rb = RunBuffer(run_id=run_id, strategy_file=str(_FAKE_STRATEGY_PATH),
                       scenario=scenario, base_dir=tmp_path)

        run(strategy_cls=strategy_cls, scenario=scenario,
            bars_by_instrument=synthetic_bars, run_buffer=rb)
        rb.finish()

        assert (tmp_path / run_id / "meta.json").exists()

    def test_fills_jsonl_exists(self, tmp_path, loaded_strategy, synthetic_bars):
        from engine.strategy_runtime.engine_runner import run
        from engine.strategy_runtime.run_buffer import RunBuffer, make_run_id

        _, scenario, strategy_cls = loaded_strategy
        run_id = make_run_id(str(_FAKE_STRATEGY_PATH), "1301.TSE")
        rb = RunBuffer(run_id=run_id, strategy_file=str(_FAKE_STRATEGY_PATH),
                       scenario=scenario, base_dir=tmp_path)

        run(strategy_cls=strategy_cls, scenario=scenario,
            bars_by_instrument=synthetic_bars, run_buffer=rb)
        rb.finish()

        assert (tmp_path / run_id / "fills.jsonl").exists()

    def test_equity_jsonl_exists(self, tmp_path, loaded_strategy, synthetic_bars):
        from engine.strategy_runtime.engine_runner import run
        from engine.strategy_runtime.run_buffer import RunBuffer, make_run_id

        _, scenario, strategy_cls = loaded_strategy
        run_id = make_run_id(str(_FAKE_STRATEGY_PATH), "1301.TSE")
        rb = RunBuffer(run_id=run_id, strategy_file=str(_FAKE_STRATEGY_PATH),
                       scenario=scenario, base_dir=tmp_path)

        run(strategy_cls=strategy_cls, scenario=scenario,
            bars_by_instrument=synthetic_bars, run_buffer=rb)
        rb.finish()

        assert (tmp_path / run_id / "equity.jsonl").exists()


class TestRunBufferContent:
    def _run_and_finish(self, tmp_path, loaded_strategy, synthetic_bars):
        from engine.strategy_runtime.engine_runner import run
        from engine.strategy_runtime.run_buffer import RunBuffer, make_run_id

        _, scenario, strategy_cls = loaded_strategy
        run_id = make_run_id(str(_FAKE_STRATEGY_PATH), "1301.TSE")
        rb = RunBuffer(run_id=run_id, strategy_file=str(_FAKE_STRATEGY_PATH),
                       scenario=scenario, base_dir=tmp_path)

        run(strategy_cls=strategy_cls, scenario=scenario,
            bars_by_instrument=synthetic_bars, run_buffer=rb)
        rb.finish()
        return run_id

    def test_meta_status_finished(self, tmp_path, loaded_strategy, synthetic_bars):
        import json
        run_id = self._run_and_finish(tmp_path, loaded_strategy, synthetic_bars)
        meta = json.loads((tmp_path / run_id / "meta.json").read_text())
        assert meta["status"] == "finished"

    def test_fills_count_gte_1(self, tmp_path, loaded_strategy, synthetic_bars):
        import json
        run_id = self._run_and_finish(tmp_path, loaded_strategy, synthetic_bars)
        fills_text = (tmp_path / run_id / "fills.jsonl").read_text()
        rows = [json.loads(l) for l in fills_text.splitlines() if l.strip()]
        assert len(rows) >= 1, f"FakeMarketBuyOnce must produce ≥1 fill, got {len(rows)}"

    def test_equity_points_equals_bar_count(self, tmp_path, loaded_strategy, synthetic_bars):
        import json
        run_id = self._run_and_finish(tmp_path, loaded_strategy, synthetic_bars)
        equity_text = (tmp_path / run_id / "equity.jsonl").read_text()
        rows = [json.loads(l) for l in equity_text.splitlines() if l.strip()]
        bar_count = sum(len(bars) for bars in synthetic_bars.values())
        assert len(rows) == bar_count, (
            f"equity_points={len(rows)} should equal bar_count={bar_count}"
        )

    def test_compute_summary_works(self, tmp_path, loaded_strategy, synthetic_bars):
        from engine.strategy_runtime.summary import compute_summary
        run_id = self._run_and_finish(tmp_path, loaded_strategy, synthetic_bars)
        run_dir = tmp_path / run_id
        summary = compute_summary(run_dir)
        assert "fee_total" in summary
        assert "fills_count" in summary
        assert "equity_points" in summary
        assert summary["fills_count"] >= 1
        bar_count = sum(len(bars) for bars in synthetic_bars.values())
        assert summary["equity_points"] == bar_count

    def test_fill_has_required_keys(self, tmp_path, loaded_strategy, synthetic_bars):
        import json
        run_id = self._run_and_finish(tmp_path, loaded_strategy, synthetic_bars)
        row = json.loads((tmp_path / run_id / "fills.jsonl").read_text().splitlines()[0])
        for key in ("instrument_id", "side", "qty", "price", "ts_event_ms"):
            assert key in row, f"fill missing required key: {key}"

    def test_equity_has_required_keys(self, tmp_path, loaded_strategy, synthetic_bars):
        import json
        run_id = self._run_and_finish(tmp_path, loaded_strategy, synthetic_bars)
        row = json.loads((tmp_path / run_id / "equity.jsonl").read_text().splitlines()[0])
        assert "ts_event_ms" in row
        assert "equity" in row
