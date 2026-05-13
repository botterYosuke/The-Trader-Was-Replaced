"""Unit tests for engine.strategy_replay.cli (Step 7)."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

_FIXTURE_DIR = Path(__file__).parent.parent / "fixtures" / "strategies"
_FAKE_STRATEGY_PATH = _FIXTURE_DIR / "fake_market_buy_once.py"


class TestCliHelp:
    def test_run_help_exits_zero(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0

    def test_run_help_contains_strategy_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
        )
        assert "--strategy" in result.stdout

    def test_run_help_contains_catalog_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
        )
        assert "--catalog" in result.stdout

    def test_run_help_contains_bars_json_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
        )
        assert "--bars-json" in result.stdout

    def test_top_level_help_exits_zero(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "--help"],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0


class TestCliMissingArgs:
    def test_missing_strategy_exits_nonzero(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run"],
            capture_output=True,
            text=True,
        )
        assert result.returncode != 0

    def test_nonexistent_strategy_exits_1(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run",
             "--strategy", "no_such_file.py",
             "--bars-json", "no_such.json"],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 1


class TestCliParserUnit:
    def test_build_parser_not_raises(self):
        from engine.strategy_replay.cli import _build_parser
        parser = _build_parser()
        assert parser is not None

    def test_parse_strategy_params_single(self):
        from engine.strategy_replay.cli import _parse_strategy_params
        result = _parse_strategy_params(["window=10"])
        assert result == {"window": "10"}

    def test_parse_strategy_params_multiple(self):
        from engine.strategy_replay.cli import _parse_strategy_params
        result = _parse_strategy_params(["window=5", "k=2.5"])
        assert result == {"window": "5", "k": "2.5"}

    def test_parse_strategy_params_empty(self):
        from engine.strategy_replay.cli import _parse_strategy_params
        assert _parse_strategy_params([]) == {}

    def test_parse_strategy_params_bad_format_raises(self):
        import argparse
        from engine.strategy_replay.cli import _parse_strategy_params
        with pytest.raises(argparse.ArgumentTypeError):
            _parse_strategy_params(["no_equals_sign"])


class TestCliBarsJsonSmoke:
    """Smoke test: CLI run with --bars-json (no real catalog needed)."""

    def _make_bars_json(self, tmp_path: Path) -> Path:
        bars = [
            {
                "ts_event": i * 86_400_000_000_000,
                "ts_init": i * 86_400_000_000_000,
                "open": str(1000.0 + i * 10),
                "high": str(1010.0 + i * 10),
                "low": str(990.0 + i * 10),
                "close": str(1005.0 + i * 10),
                "volume": "1000",
                "granularity": "Daily",
            }
            for i in range(5)
        ]
        p = tmp_path / "bars.json"
        p.write_text(json.dumps({"1301.TSE": bars}), encoding="utf-8")
        return p

    def test_run_with_bars_json_exits_zero(self, tmp_path):
        bars_json = self._make_bars_json(tmp_path)
        result = subprocess.run(
            [
                sys.executable, "-m", "engine.strategy_replay", "run",
                "--strategy", str(_FAKE_STRATEGY_PATH),
                "--bars-json", str(bars_json),
                "--run-buffer-dir", str(tmp_path / "rb"),
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, f"stderr: {result.stderr}"

    def test_run_with_bars_json_prints_run_id(self, tmp_path):
        bars_json = self._make_bars_json(tmp_path)
        result = subprocess.run(
            [
                sys.executable, "-m", "engine.strategy_replay", "run",
                "--strategy", str(_FAKE_STRATEGY_PATH),
                "--bars-json", str(bars_json),
                "--run-buffer-dir", str(tmp_path / "rb"),
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0
        out = json.loads(result.stdout)
        assert "run_id" in out
        assert "fills_count" in out
        assert "equity_points" in out

    def test_run_with_bars_json_creates_run_buffer_files(self, tmp_path):
        bars_json = self._make_bars_json(tmp_path)
        rb_dir = tmp_path / "rb"
        subprocess.run(
            [
                sys.executable, "-m", "engine.strategy_replay", "run",
                "--strategy", str(_FAKE_STRATEGY_PATH),
                "--bars-json", str(bars_json),
                "--run-buffer-dir", str(rb_dir),
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        run_dirs = [d for d in rb_dir.iterdir() if d.is_dir()]
        assert len(run_dirs) == 1
        run_dir = run_dirs[0]
        assert (run_dir / "meta.json").exists()
        assert (run_dir / "fills.jsonl").exists()
        assert (run_dir / "equity.jsonl").exists()
        assert (run_dir / "summary.json").exists()
