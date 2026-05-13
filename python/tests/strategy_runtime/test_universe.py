"""Tests for engine.strategy_runtime.universe."""
from __future__ import annotations

from pathlib import Path

import pytest

from engine.strategy_runtime.universe import resolve_universe_json_path


def test_absolute_path_returned_unchanged(tmp_path):
    abs_json = tmp_path / "universe.json"
    abs_json.touch()
    strategy = tmp_path / "some_strategy.py"
    result = resolve_universe_json_path(strategy, str(abs_json))
    assert result == abs_json


def test_relative_path_resolved_from_strategy_parent(tmp_path):
    # strategy is at tmp_path/strategies/my_strategy.py
    strategies_dir = tmp_path / "strategies"
    strategies_dir.mkdir()
    strategy = strategies_dir / "my_strategy.py"

    # universe JSON is at tmp_path/data/universe/foo.json
    data_dir = tmp_path / "data" / "universe"
    data_dir.mkdir(parents=True)
    universe_json = data_dir / "foo.json"
    universe_json.touch()

    result = resolve_universe_json_path(strategy, "../data/universe/foo.json")
    assert result == universe_json.resolve()


def test_relative_path_parent_traversal(tmp_path):
    # strategy at <tmp>/b/strategy.py, universe at <tmp>/c/d.json
    b_dir = tmp_path / "b"
    b_dir.mkdir()
    strategy = b_dir / "strategy.py"
    result = resolve_universe_json_path(strategy, "../c/d.json")
    expected = (tmp_path / "c" / "d.json").resolve()
    assert result == expected


def test_returns_path_object(tmp_path):
    strategy = tmp_path / "s.py"
    result = resolve_universe_json_path(strategy, "relative/path.json")
    assert isinstance(result, Path)
