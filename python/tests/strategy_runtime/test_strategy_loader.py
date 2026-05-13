"""Unit tests for engine.strategy_runtime.strategy_loader.

Tests that require the real blacksheep strategy files are skipped when those
files are not present.
"""

from __future__ import annotations

import os
import textwrap
from pathlib import Path
from types import ModuleType

import pytest

from engine.strategy_runtime.strategy_loader import (
    StrategyLoadError,
    get_strategy_param_env,
    load,
)

# ---------------------------------------------------------------------------
# Paths to real blacksheep strategies
# ---------------------------------------------------------------------------

_BLACKSHEEP = Path(r"C:\Users\sasai\Documents\🐃_blacksheep")
_MR01 = _BLACKSHEEP / "strategies" / "mean_reversion_01.py"
_OF06 = _BLACKSHEEP / "strategies" / "order_flow_06.py"

requires_mr01 = pytest.mark.skipif(not _MR01.exists(), reason="mean_reversion_01.py not found")
requires_of06 = pytest.mark.skipif(not _OF06.exists(), reason="order_flow_06.py not found")


# ---------------------------------------------------------------------------
# load — real files
# ---------------------------------------------------------------------------


@requires_mr01
def test_load_mean_reversion_01_returns_tuple():
    module, scenario, strategy_cls = load(_MR01)
    assert isinstance(module, ModuleType)
    assert isinstance(scenario, dict)
    assert scenario["schema_version"] == 1


@requires_mr01
def test_load_mean_reversion_01_strategy_cls_is_strategy_subclass():
    from nautilus_trader.trading.strategy import Strategy
    _module, _scenario, strategy_cls = load(_MR01)
    assert issubclass(strategy_cls, Strategy)
    assert strategy_cls.__name__ == "MeanReversion01Strategy"


@requires_of06
def test_load_order_flow_06_returns_tuple():
    module, scenario, strategy_cls = load(_OF06)
    assert isinstance(module, ModuleType)
    assert isinstance(scenario, dict)


@requires_of06
def test_load_order_flow_06_scenario_has_instruments_after_resolve():
    _module, scenario, _cls = load(_OF06)
    assert "instruments" in scenario
    assert isinstance(scenario["instruments"], list)
    assert len(scenario["instruments"]) > 0


@requires_of06
def test_load_order_flow_06_strategy_cls_is_strategy_subclass():
    from nautilus_trader.trading.strategy import Strategy
    _module, _scenario, strategy_cls = load(_OF06)
    assert issubclass(strategy_cls, Strategy)


# ---------------------------------------------------------------------------
# load — synthetic fixtures
# ---------------------------------------------------------------------------


def _write_strategy(tmp_path: Path, extra: str = "") -> Path:
    src = textwrap.dedent(
        """\
        from nautilus_trader.config import StrategyConfig
        from nautilus_trader.trading.strategy import Strategy

        SCENARIO = {
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-01-10",
            "granularity": "Minute",
            "initial_cash": 1000000,
        }

        class FakeStrategy(Strategy):
            def __init__(self, **kwargs):
                super().__init__(config=StrategyConfig(strategy_id="fake-01"))
        """
    )
    if extra:
        src += "\n" + textwrap.dedent(extra)
    p = tmp_path / "fake_strategy.py"
    p.write_text(src, encoding="utf-8")
    return p


def test_load_synthetic_returns_correct_types(tmp_path: Path):
    from nautilus_trader.trading.strategy import Strategy
    p = _write_strategy(tmp_path)
    module, scenario, strategy_cls = load(p)
    assert isinstance(module, ModuleType)
    assert scenario["schema_version"] == 1
    assert issubclass(strategy_cls, Strategy)
    assert strategy_cls.__name__ == "FakeStrategy"


def test_load_raises_file_not_found():
    with pytest.raises(FileNotFoundError):
        load(Path("/nonexistent/strategy.py"))


def test_load_raises_when_no_strategy_subclass(tmp_path: Path):
    p = tmp_path / "no_strategy.py"
    p.write_text(
        textwrap.dedent("""\
        SCENARIO = {
            "schema_version": 1, "instrument": "X.TSE",
            "start": "2025-01-06", "end": "2025-01-10",
            "granularity": "Minute", "initial_cash": 1,
        }
        class NotAStrategy:
            pass
        """),
        encoding="utf-8",
    )
    with pytest.raises(StrategyLoadError, match="no Strategy subclass"):
        load(p)


def test_load_raises_when_no_scenario(tmp_path: Path):
    p = tmp_path / "no_scenario.py"
    p.write_text(
        textwrap.dedent("""\
        from nautilus_trader.config import StrategyConfig
        from nautilus_trader.trading.strategy import Strategy
        class FakeStrategy(Strategy):
            def __init__(self, **kwargs):
                super().__init__(config=StrategyConfig(strategy_id="fake"))
        """),
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="SCENARIO not found"):
        load(p)


# ---------------------------------------------------------------------------
# get_strategy_param_env
# ---------------------------------------------------------------------------


def test_get_strategy_param_env_empty(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.delenv("STRATEGY_PARAM_HOLDING_MINUTES", raising=False)
    monkeypatch.delenv("STRATEGY_PARAM_WINDOW", raising=False)
    result = get_strategy_param_env()
    assert "holding_minutes" not in result
    assert "window" not in result


def test_get_strategy_param_env_single(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("STRATEGY_PARAM_HOLDING_MINUTES", "42")
    result = get_strategy_param_env()
    assert result["holding_minutes"] == "42"


def test_get_strategy_param_env_multiple(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("STRATEGY_PARAM_HOLDING_MINUTES", "42")
    monkeypatch.setenv("STRATEGY_PARAM_WINDOW", "10")
    result = get_strategy_param_env()
    assert result["holding_minutes"] == "42"
    assert result["window"] == "10"


def test_get_strategy_param_env_key_is_lowercased(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("STRATEGY_PARAM_KILL_HOLDING_CAP", "1")
    result = get_strategy_param_env()
    assert "kill_holding_cap" in result
    assert result["kill_holding_cap"] == "1"


def test_get_strategy_param_env_non_prefixed_not_included(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("HOLDING_MINUTES", "99")
    result = get_strategy_param_env()
    assert "holding_minutes" not in result
