"""Tests for RunBuffer schema and file output (Step 4)."""
from __future__ import annotations

import json
from pathlib import Path

from engine.strategy_runtime.run_buffer import RunBuffer, get_run_buffer_base_dir, make_run_id


def _make_buffer(tmp_path: Path, run_id: str = "test-run-001") -> RunBuffer:
    return RunBuffer(
        run_id=run_id,
        strategy_file="fake_strategy.py",
        scenario={"instrument": "1301.TSE", "granularity": "Daily"},
        base_dir=tmp_path,
    )


class TestRunBufferFiles:
    def test_meta_created_on_init(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.abort()
        assert (tmp_path / "test-run-001" / "meta.json").exists()

    def test_meta_schema_version(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.abort()
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        assert meta["schema_version"] == 1

    def test_meta_required_keys(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.abort()
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        for key in ("run_id", "strategy_file", "strategy_sha256", "git_rev",
                    "scenario", "started_at", "finished_at", "status"):
            assert key in meta, f"meta missing key: {key}"

    def test_finish_sets_status_finished(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.finish()
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        assert meta["status"] == "finished"

    def test_abort_sets_status_aborted(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.abort()
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        assert meta["status"] == "aborted"

    def test_finish_sets_finished_at(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.finish()
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        assert meta["finished_at"] is not None

    def test_fills_jsonl_written(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.write_fill({"instrument_id": "1301.TSE", "side": "BUY", "qty": "100",
                        "price": "1000", "ts_event_ms": 1_700_000_000_000})
        rb.finish()
        fills_path = tmp_path / "test-run-001" / "fills.jsonl"
        assert fills_path.exists()
        rows = [json.loads(l) for l in fills_path.read_text().splitlines() if l]
        assert len(rows) == 1
        assert rows[0]["side"] == "BUY"

    def test_equity_jsonl_written(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.write_equity({"ts_event_ms": 1_700_000_000_000, "equity": 10_500_000.0})
        rb.finish()
        equity_path = tmp_path / "test-run-001" / "equity.jsonl"
        assert equity_path.exists()
        rows = [json.loads(l) for l in equity_path.read_text().splitlines() if l]
        assert len(rows) == 1
        assert rows[0]["equity"] == 10_500_000.0

    def test_commission_preserved_in_fill(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.write_fill({"instrument_id": "1301.TSE", "side": "BUY", "qty": "100",
                        "price": "1000", "ts_event_ms": 0, "commission": "55.5"})
        rb.finish()
        fills_path = tmp_path / "test-run-001" / "fills.jsonl"
        row = json.loads(fills_path.read_text().splitlines()[0])
        assert row["commission"] == "55.5"

    def test_all_three_files_present_after_finish(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.write_fill({"instrument_id": "1301.TSE", "side": "BUY",
                        "qty": "100", "price": "1000", "ts_event_ms": 0})
        rb.write_equity({"ts_event_ms": 0, "equity": 10_000_000.0})
        rb.finish()
        run_dir = tmp_path / "test-run-001"
        assert (run_dir / "meta.json").exists()
        assert (run_dir / "fills.jsonl").exists()
        assert (run_dir / "equity.jsonl").exists()

    def test_context_manager_abort_on_exception(self, tmp_path):
        try:
            with _make_buffer(tmp_path) as rb:
                rb.write_equity({"ts_event_ms": 0, "equity": 1.0})
                raise RuntimeError("simulated failure")
        except RuntimeError:
            pass
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        assert meta["status"] == "aborted"

    def test_run_dir_property(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.abort()
        assert rb.run_dir == tmp_path / "test-run-001"

    def test_finish_idempotent(self, tmp_path):
        rb = _make_buffer(tmp_path)
        rb.finish()
        rb.finish()  # must not raise
        meta = json.loads((tmp_path / "test-run-001" / "meta.json").read_text())
        assert meta["status"] == "finished"


class TestRunBufferHelpers:
    def test_make_run_id_format(self):
        run_id = make_run_id("strategy/mean_reversion_01.py", "1301.TSE")
        parts = run_id.split("-")
        # format: {utc_sec}-{stem}-{instrument_clean}  → 3 parts
        assert len(parts) == 3
        assert parts[1] == "mean_reversion_01"
        assert parts[2] == "1301_TSE"

    def test_make_run_id_dot_replaced(self):
        run_id = make_run_id("s.py", "1301.TSE")
        assert "." not in run_id.split("-", 1)[1]

    def test_get_run_buffer_base_dir_returns_path(self):
        d = get_run_buffer_base_dir()
        assert isinstance(d, Path)
        assert "flowsurface" in str(d)
