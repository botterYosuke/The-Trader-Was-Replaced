"""Unit tests for engine.strategy_replay.cli (Step 7)."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from decimal import Decimal
from pathlib import Path

import pytest

_FIXTURE_DIR = Path(__file__).parent.parent / "fixtures" / "strategies"
_FAKE_STRATEGY_PATH = _FIXTURE_DIR / "fake_market_buy_once.py"
_MR01_PATH = Path("C:/Users/sasai/Documents/🐃_blacksheep/strategies/mean_reversion_01.py")
_CATALOG_PATH = Path(
    os.environ.get(
        "JQUANTS_CATALOG_PATH",
        "C:/Users/sasai/Documents/The-Trader-Was-Replaced/artifacts/jquants-catalog",
    )
)

_PYTHON_SRC = Path(__file__).parent.parent.parent  # → python/

_DAY_NS = 86_400_000_000_000


def _cli_env() -> dict:
    import os
    env = os.environ.copy()
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = str(_PYTHON_SRC) + (os.pathsep + existing if existing else "")
    return env


def _make_fake_bars_by_instrument():
    """Return {InstrumentId: list[Bar]} with 5 synthetic Daily bars for 1301.TSE."""
    from nautilus_trader.model.data import Bar, BarSpecification, BarType
    from nautilus_trader.model.enums import AggregationSource, BarAggregation, PriceType
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.objects import Price, Quantity

    iid = InstrumentId.from_str("1301.TSE")
    bar_spec = BarSpecification(1, BarAggregation.DAY, PriceType.LAST)
    bar_type = BarType(iid, bar_spec, AggregationSource.EXTERNAL)
    bars = []
    for i in range(5):
        ts = i * _DAY_NS
        close = Price(Decimal(str(1000.0 + i * 10)), precision=1)
        bars.append(Bar(
            bar_type=bar_type,
            open=close,
            high=Price(Decimal(str(1010.0 + i * 10)), precision=1),
            low=Price(Decimal(str(990.0 + i * 10)), precision=1),
            close=close,
            volume=Quantity(1000, precision=0),
            ts_event=ts,
            ts_init=ts,
        ))
    return {iid: bars}


def _default_run_args(tmp_path: Path, **overrides) -> argparse.Namespace:
    """Build a minimal Namespace for _cmd_run / run_command."""
    defaults = dict(
        strategy=str(_FAKE_STRATEGY_PATH),
        catalog=None,
        bars_json=None,
        run_buffer_dir=str(tmp_path / "rb"),
        strategy_params=[],
        granularity=None,
        start=None,
        end=None,
        verbose=False,
    )
    defaults.update(overrides)
    return argparse.Namespace(**defaults)


class TestCliHelp:
    def test_run_help_exits_zero(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
            env=_cli_env(),
        )
        assert result.returncode == 0

    def test_run_help_contains_strategy_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
            env=_cli_env(),
        )
        assert "--strategy" in result.stdout

    def test_run_help_contains_catalog_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
            env=_cli_env(),
        )
        assert "--catalog" in result.stdout

    def test_run_help_contains_bars_json_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True,
            text=True,
            env=_cli_env(),
        )
        assert "--bars-json" in result.stdout

    def test_top_level_help_exits_zero(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "--help"],
            capture_output=True,
            text=True,
            env=_cli_env(),
        )
        assert result.returncode == 0


class TestCliMissingArgs:
    def test_missing_strategy_exits_nonzero(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run"],
            capture_output=True,
            text=True,
            env=_cli_env(),
        )
        assert result.returncode != 0

    def test_nonexistent_strategy_exits_1(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run",
             "--strategy", "no_such_file.py",
             "--bars-json", "no_such.json"],
            capture_output=True,
            text=True,
            env=_cli_env(),
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
            env=_cli_env(),
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
            env=_cli_env(),
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
            env=_cli_env(),
            check=True,
        )
        run_dirs = [d for d in rb_dir.iterdir() if d.is_dir()]
        assert len(run_dirs) == 1
        run_dir = run_dirs[0]
        assert (run_dir / "meta.json").exists()
        assert (run_dir / "fills.jsonl").exists()
        assert (run_dir / "equity.jsonl").exists()
        assert (run_dir / "summary.json").exists()


class TestCliCatalogUnit:
    """--catalog 経路の unit test。_load_bars_from_catalog を monkeypatch する。"""

    def test_catalog_route_exits_zero(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod
        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog",
                            lambda catalog_dir, scenario: _make_fake_bars_by_instrument())

        from engine.strategy_replay.cli import run_command
        args = _default_run_args(tmp_path, catalog="/fake/catalog")
        assert run_command(args) == 0

    def test_catalog_route_creates_run_buffer_files(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod
        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog",
                            lambda catalog_dir, scenario: _make_fake_bars_by_instrument())

        from engine.strategy_replay.cli import run_command
        rb_dir = tmp_path / "rb"
        args = _default_run_args(tmp_path, catalog="/fake/catalog",
                                  run_buffer_dir=str(rb_dir))
        run_command(args)

        run_dirs = [d for d in rb_dir.iterdir() if d.is_dir()]
        assert len(run_dirs) == 1
        run_dir = run_dirs[0]
        assert (run_dir / "meta.json").exists()
        assert (run_dir / "fills.jsonl").exists()
        assert (run_dir / "equity.jsonl").exists()
        assert (run_dir / "summary.json").exists()

    def test_catalog_route_meta_status_finished(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod
        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog",
                            lambda catalog_dir, scenario: _make_fake_bars_by_instrument())

        from engine.strategy_replay.cli import run_command
        rb_dir = tmp_path / "rb"
        args = _default_run_args(tmp_path, catalog="/fake/catalog",
                                  run_buffer_dir=str(rb_dir))
        run_command(args)

        run_dir = next((rb_dir / d).parent / d
                       for d in [p.name for p in rb_dir.iterdir() if p.is_dir()])
        meta = json.loads((run_dir / "meta.json").read_text())
        assert meta["status"] == "finished"

    def test_catalog_route_equity_points_eq_bar_count(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod
        fake_bars = _make_fake_bars_by_instrument()
        bar_count = sum(len(v) for v in fake_bars.values())
        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog",
                            lambda catalog_dir, scenario: fake_bars)

        from engine.strategy_replay.cli import run_command
        rb_dir = tmp_path / "rb"
        args = _default_run_args(tmp_path, catalog="/fake/catalog",
                                  run_buffer_dir=str(rb_dir))
        result_code = run_command(args)
        assert result_code == 0

        run_dir = next(p for p in rb_dir.iterdir() if p.is_dir())
        summary = json.loads((run_dir / "summary.json").read_text())
        assert summary["equity_points"] == bar_count

    def test_catalog_returns_none_exits_1(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod
        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog",
                            lambda catalog_dir, scenario: None)

        from engine.strategy_replay.cli import run_command
        args = _default_run_args(tmp_path, catalog="/broken/catalog")
        assert run_command(args) == 1

    def test_no_source_exits_1(self, tmp_path):
        """--catalog も --bars-json もない場合は exit 1。"""
        from engine.strategy_replay.cli import run_command
        args = _default_run_args(tmp_path)  # catalog=None, bars_json=None
        assert run_command(args) == 1


class TestCliGranularityOverride:
    """--granularity フラグが scenario を上書きすることを確認する。"""

    def test_granularity_override_passed_to_loader(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod

        received: list[dict] = []

        def capture_load(catalog_dir, scenario):
            received.append(dict(scenario))
            return _make_fake_bars_by_instrument()

        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog", capture_load)

        from engine.strategy_replay.cli import run_command
        # fake_market_buy_once has SCENARIO granularity=Daily; override to Daily
        # (same value — just verify the key survives the override path)
        args = _default_run_args(tmp_path, catalog="/fake/catalog",
                                  granularity="Daily")
        run_command(args)

        assert received, "loader was never called"
        assert received[0]["granularity"] == "Daily"

    def test_invalid_granularity_exits_1(self, tmp_path, monkeypatch):
        import engine.strategy_replay.cli as cli_mod
        monkeypatch.setattr(cli_mod, "_load_bars_from_catalog",
                            lambda *a: _make_fake_bars_by_instrument())

        from engine.strategy_replay.cli import run_command
        args = _default_run_args(tmp_path, catalog="/fake/catalog",
                                  granularity="Weekly")
        assert run_command(args) == 1

    def test_help_contains_granularity_flag(self):
        result = subprocess.run(
            [sys.executable, "-m", "engine.strategy_replay", "run", "--help"],
            capture_output=True, text=True,
            env=_cli_env(),
        )
        assert "--granularity" in result.stdout


@pytest.mark.slow
@pytest.mark.skipif(
    not _MR01_PATH.exists(),
    reason=f"blacksheep strategy not found: {_MR01_PATH}",
)
@pytest.mark.skipif(
    not _CATALOG_PATH.exists(),
    reason=f"catalog not found (set JQUANTS_CATALOG_PATH or place at {_CATALOG_PATH})",
)
class TestCliCatalogSlow:
    """実 catalog + mean_reversion_01.py を使った E2E smoke。"""

    def test_real_catalog_smoke(self, tmp_path):
        rb_dir = tmp_path / "rb"
        result = subprocess.run(
            [
                sys.executable, "-m", "engine.strategy_replay", "run",
                "--strategy", str(_MR01_PATH),
                "--catalog", str(_CATALOG_PATH),
                "--run-buffer-dir", str(rb_dir),
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, (
            f"CLI failed\nstdout: {result.stdout}\nstderr: {result.stderr}"
        )

    def test_real_catalog_equity_points_gt_zero(self, tmp_path):
        rb_dir = tmp_path / "rb"
        result = subprocess.run(
            [
                sys.executable, "-m", "engine.strategy_replay", "run",
                "--strategy", str(_MR01_PATH),
                "--catalog", str(_CATALOG_PATH),
                "--run-buffer-dir", str(rb_dir),
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0
        out = json.loads(result.stdout)
        assert out["equity_points"] > 0, f"expected equity data, got: {out}"

    def test_real_catalog_creates_run_buffer_files(self, tmp_path):
        rb_dir = tmp_path / "rb"
        subprocess.run(
            [
                sys.executable, "-m", "engine.strategy_replay", "run",
                "--strategy", str(_MR01_PATH),
                "--catalog", str(_CATALOG_PATH),
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
        assert (run_dir / "equity.jsonl").exists()
        assert (run_dir / "summary.json").exists()
        meta = json.loads((run_dir / "meta.json").read_text())
        assert meta["status"] == "finished"
