"""Tests for engine.strategy_runtime.summary (Step 4)."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from engine.strategy_runtime.summary import compute_summary, write_summary_json
from engine.summary import compute_summary as shim_compute_summary


def _write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


class TestComputeSummary:
    def test_empty_dir_returns_zero_metrics(self, tmp_path):
        result = compute_summary(tmp_path)
        assert result["total_pnl"] == 0.0
        assert result["max_drawdown"] == 0.0
        assert result["trade_count"] == 0
        assert result["win_rate"] is None
        assert result["fee_total"] == 0.0
        assert result["equity_points"] == 0
        assert result["fills_count"] == 0

    def test_equity_points_counted(self, tmp_path):
        _write_jsonl(tmp_path / "equity.jsonl", [
            {"ts_event_ms": 0, "equity": 10_000_000.0},
            {"ts_event_ms": 1, "equity": 10_100_000.0},
            {"ts_event_ms": 2, "equity": 10_050_000.0},
        ])
        result = compute_summary(tmp_path)
        assert result["equity_points"] == 3

    def test_total_pnl(self, tmp_path):
        _write_jsonl(tmp_path / "equity.jsonl", [
            {"ts_event_ms": 0, "equity": 10_000_000.0},
            {"ts_event_ms": 1, "equity": 10_500_000.0},
        ])
        result = compute_summary(tmp_path)
        assert result["total_pnl"] == pytest.approx(500_000.0)

    def test_max_drawdown(self, tmp_path):
        _write_jsonl(tmp_path / "equity.jsonl", [
            {"ts_event_ms": 0, "equity": 10_000_000.0},
            {"ts_event_ms": 1, "equity": 11_000_000.0},
            {"ts_event_ms": 2, "equity": 9_500_000.0},  # drawdown 1_500_000
        ])
        result = compute_summary(tmp_path)
        assert result["max_drawdown"] == pytest.approx(1_500_000.0)

    def test_fills_count(self, tmp_path):
        _write_jsonl(tmp_path / "fills.jsonl", [
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 0},
            {"side": "SELL", "qty": "100", "price": "1050", "ts_event_ms": 1},
        ])
        result = compute_summary(tmp_path)
        assert result["fills_count"] == 2

    def test_trade_count_and_win_rate(self, tmp_path):
        _write_jsonl(tmp_path / "fills.jsonl", [
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 0},
            {"side": "SELL", "qty": "100", "price": "1050", "ts_event_ms": 1},  # win +5000
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 2},
            {"side": "SELL", "qty": "100", "price": "980", "ts_event_ms": 3},   # loss -2000
        ])
        result = compute_summary(tmp_path)
        assert result["trade_count"] == 2
        assert result["win_rate"] == pytest.approx(0.5)

    def test_fee_total_numeric_string(self, tmp_path):
        _write_jsonl(tmp_path / "fills.jsonl", [
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 0, "commission": "55.5"},
            {"side": "SELL", "qty": "100", "price": "1050", "ts_event_ms": 1, "commission": "55.5"},
        ])
        result = compute_summary(tmp_path)
        assert result["fee_total"] == pytest.approx(111.0)

    def test_fee_total_missing_commission_is_zero(self, tmp_path):
        _write_jsonl(tmp_path / "fills.jsonl", [
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 0},
        ])
        result = compute_summary(tmp_path)
        assert result["fee_total"] == 0.0

    def test_fee_total_empty_string_commission_treated_as_missing(self, tmp_path):
        _write_jsonl(tmp_path / "fills.jsonl", [
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 0, "commission": ""},
        ])
        result = compute_summary(tmp_path)
        assert result["fee_total"] == 0.0

    def test_win_rate_none_when_no_trades(self, tmp_path):
        _write_jsonl(tmp_path / "fills.jsonl", [
            {"side": "BUY", "qty": "100", "price": "1000", "ts_event_ms": 0},
        ])
        result = compute_summary(tmp_path)
        assert result["win_rate"] is None


class TestWriteSummaryJson:
    def test_file_written(self, tmp_path):
        summary = {"total_pnl": 100.0, "trade_count": 1}
        out = write_summary_json(tmp_path, summary)
        assert out == tmp_path / "summary.json"
        assert out.exists()

    def test_content_roundtrips(self, tmp_path):
        summary = {"total_pnl": 99.5, "fee_total": 11.0, "win_rate": 0.6}
        write_summary_json(tmp_path, summary)
        loaded = json.loads((tmp_path / "summary.json").read_text())
        assert loaded == summary

    def test_creates_target_dir(self, tmp_path):
        nested = tmp_path / "a" / "b"
        write_summary_json(nested, {"x": 1})
        assert (nested / "summary.json").exists()


class TestShimImport:
    def test_shim_is_same_function(self):
        assert shim_compute_summary is compute_summary
