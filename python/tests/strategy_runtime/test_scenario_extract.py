"""Unit tests for engine.strategy_runtime.scenario.

Tests run against the real blacksheep strategy files. If those files are not
present the tests are skipped (they live outside this repo).
"""

from __future__ import annotations

import json
import logging
import textwrap
from pathlib import Path

import pytest

from engine.strategy_runtime.scenario import (
    ScenarioValidationError,
    extract,
    load_scenario,
    normalize_scenario,
    resolve_refs,
    validate,
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
# extract — real files
# ---------------------------------------------------------------------------


@requires_mr01
def test_extract_mean_reversion_01_returns_dict():
    sc = extract(_MR01)
    assert isinstance(sc, dict)


@requires_mr01
def test_extract_mean_reversion_01_schema_v1():
    sc = extract(_MR01)
    assert sc is not None
    assert sc["schema_version"] == 1
    assert "instrument" in sc
    assert isinstance(sc["instrument"], str)


@requires_mr01
def test_extract_mean_reversion_01_required_keys():
    sc = extract(_MR01)
    assert sc is not None
    for key in ("start", "end", "granularity", "initial_cash"):
        assert key in sc, f"missing key: {key}"


@requires_of06
def test_extract_order_flow_06_returns_dict():
    sc = extract(_OF06)
    assert isinstance(sc, dict)


@requires_of06
def test_extract_order_flow_06_schema_v3_with_instruments_ref():
    sc = extract(_OF06)
    assert sc is not None
    assert sc["schema_version"] == 3
    assert "instruments_ref" in sc
    assert isinstance(sc["instruments_ref"], str)


# ---------------------------------------------------------------------------
# resolve_refs — real files (requires universe JSON)
# ---------------------------------------------------------------------------


@requires_of06
def test_resolve_refs_order_flow_06_produces_instruments_list():
    sc = extract(_OF06)
    assert sc is not None
    resolved = resolve_refs(sc, base_dir=_OF06.parent)
    assert "instruments" in resolved
    assert isinstance(resolved["instruments"], list)
    assert len(resolved["instruments"]) > 0
    assert all(isinstance(x, str) for x in resolved["instruments"])


@requires_of06
def test_resolve_refs_preserves_instruments_ref_key():
    sc = extract(_OF06)
    assert sc is not None
    resolved = resolve_refs(sc, base_dir=_OF06.parent)
    assert "instruments_ref" in resolved


# ---------------------------------------------------------------------------
# validate — real files
# ---------------------------------------------------------------------------


@requires_mr01
def test_validate_mean_reversion_01_passes():
    sc = extract(_MR01)
    assert sc is not None
    validate(sc)  # should not raise


@requires_of06
def test_validate_order_flow_06_passes_after_resolve():
    sc = extract(_OF06)
    assert sc is not None
    resolved = resolve_refs(sc, base_dir=_OF06.parent)
    validate(resolved)  # should not raise


# ---------------------------------------------------------------------------
# extract — synthetic fixtures (no external files needed)
# ---------------------------------------------------------------------------


def _write_py(tmp_path: Path, src: str) -> Path:
    p = tmp_path / "strategy.py"
    p.write_text(textwrap.dedent(src), encoding="utf-8")
    return p


def test_extract_v1_inline(tmp_path: Path):
    p = _write_py(
        tmp_path,
        """\
        SCENARIO = {
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-01-10",
            "granularity": "Minute",
            "initial_cash": 1000000,
        }
        """,
    )
    sc = extract(p)
    assert sc is not None
    assert sc["schema_version"] == 1
    assert sc["instrument"] == "1301.TSE"


def test_extract_v3_instruments_ref_inline(tmp_path: Path):
    p = _write_py(
        tmp_path,
        """\
        SCENARIO = {
            "schema_version": 3,
            "instruments_ref": "universe.json#/instruments",
            "start": "2025-01-06",
            "end": "2025-01-10",
            "granularity": "Minute",
            "initial_cash": 1000000,
        }
        """,
    )
    sc = extract(p)
    assert sc is not None
    assert sc["schema_version"] == 3
    assert sc["instruments_ref"] == "universe.json#/instruments"


def test_extract_returns_none_when_no_scenario(tmp_path: Path):
    p = _write_py(tmp_path, "x = 1\n")
    assert extract(p) is None


def test_extract_raises_on_comprehension(tmp_path: Path):
    p = _write_py(
        tmp_path,
        """\
        SCENARIO = {k: v for k, v in [("schema_version", 1)]}
        """,
    )
    with pytest.raises(ValueError):
        extract(p)


def test_extract_raises_on_multiple_scenario(tmp_path: Path):
    p = _write_py(
        tmp_path,
        """\
        SCENARIO = {"schema_version": 1, "instrument": "A.TSE",
                    "start": "2025-01-06", "end": "2025-01-10",
                    "granularity": "Minute", "initial_cash": 1}
        SCENARIO = {"schema_version": 1, "instrument": "B.TSE",
                    "start": "2025-01-06", "end": "2025-01-10",
                    "granularity": "Minute", "initial_cash": 1}
        """,
    )
    with pytest.raises(ScenarioValidationError):
        extract(p)


# ---------------------------------------------------------------------------
# resolve_refs — synthetic fixtures
# ---------------------------------------------------------------------------


def test_resolve_refs_v1_is_noop(tmp_path: Path):
    sc = {"schema_version": 1, "instrument": "1301.TSE",
          "start": "2025-01-06", "end": "2025-01-10",
          "granularity": "Minute", "initial_cash": 1_000_000}
    result = resolve_refs(sc, base_dir=tmp_path)
    assert result == sc


def test_resolve_refs_v3_with_local_json(tmp_path: Path):
    universe = {"instruments": ["1301.TSE", "1332.TSE"]}
    (tmp_path / "universe.json").write_text(json.dumps(universe), encoding="utf-8")

    sc = {
        "schema_version": 3,
        "instruments_ref": "universe.json#/instruments",
        "start": "2025-01-06",
        "end": "2025-01-10",
        "granularity": "Minute",
        "initial_cash": 1_000_000,
    }
    result = resolve_refs(sc, base_dir=tmp_path)
    assert result["instruments"] == ["1301.TSE", "1332.TSE"]
    assert "instruments_ref" in result  # key preserved


def test_resolve_refs_v3_missing_json_raises(tmp_path: Path):
    sc = {
        "schema_version": 3,
        "instruments_ref": "no_such_file.json#/instruments",
        "start": "2025-01-06",
        "end": "2025-01-10",
        "granularity": "Minute",
        "initial_cash": 1_000_000,
    }
    with pytest.raises(ScenarioValidationError) as exc_info:
        resolve_refs(sc, base_dir=tmp_path)
    assert exc_info.value.code == "unresolved_ref"


# ---------------------------------------------------------------------------
# validate — synthetic fixtures
# ---------------------------------------------------------------------------


def test_validate_v1_ok():
    validate({
        "schema_version": 1,
        "instrument": "1301.TSE",
        "start": "2025-01-06",
        "end": "2025-01-10",
        "granularity": "Minute",
        "initial_cash": 1_000_000,
    })


def test_validate_v1_missing_key():
    with pytest.raises(ScenarioValidationError):
        validate({"schema_version": 1, "instrument": "1301.TSE"})


def test_validate_v1_extra_key():
    with pytest.raises(ScenarioValidationError):
        validate({
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-01-10",
            "granularity": "Minute",
            "initial_cash": 1_000_000,
            "unknown_key": "oops",
        })


def test_validate_v1_bool_rejected_as_initial_cash():
    with pytest.raises(ScenarioValidationError):
        validate({
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-01-10",
            "granularity": "Minute",
            "initial_cash": True,
        })


def test_validate_unknown_schema_version():
    with pytest.raises(ScenarioValidationError):
        validate({"schema_version": 99})


# ---------------------------------------------------------------------------
# load_scenario — synthetic fixtures
# ---------------------------------------------------------------------------


def test_load_scenario_prefers_sidecar(tmp_path: Path):
    py = tmp_path / "strat.py"
    py.write_text("# no SCENARIO here")
    sidecar = tmp_path / "strat.json"
    sidecar.write_text(json.dumps({
        "scenario": {
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000,
        }
    }))
    d = load_scenario(py)
    assert d["instrument"] == "1301.TSE"


def test_load_scenario_falls_back_to_py_with_warning(tmp_path: Path, caplog):
    py = tmp_path / "strat.py"
    py.write_text(textwrap.dedent("""
        SCENARIO = {
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000,
        }
    """))
    with caplog.at_level(logging.WARNING):
        d = load_scenario(py)
    assert "legacy" in caplog.text
    assert d["instrument"] == "1301.TSE"


def test_load_scenario_raises_when_both_absent(tmp_path: Path):
    py = tmp_path / "strat.py"
    py.write_text("# no SCENARIO")
    with pytest.raises(ValueError):
        load_scenario(py)


def test_load_scenario_invalid_json_raises(tmp_path: Path):
    py = tmp_path / "strat.py"
    py.write_text("# no SCENARIO")
    sidecar = tmp_path / "strat.json"
    sidecar.write_text("{not valid json")
    with pytest.raises(ScenarioValidationError):
        load_scenario(py)


def test_load_scenario_layout_only_json_falls_back_to_py(tmp_path: Path):
    py = tmp_path / "strat.py"
    py.write_text(textwrap.dedent("""
        SCENARIO = {
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000,
        }
    """))
    sidecar = tmp_path / "strat.json"
    sidecar.write_text(json.dumps({"schema_version": 1, "viewport": {}, "windows": []}))
    d = load_scenario(py)
    assert d["instrument"] == "1301.TSE"


def test_load_scenario_normalizes_v2_instrument_key(tmp_path: Path):
    py = tmp_path / "strat.py"
    py.write_text(textwrap.dedent("""
        SCENARIO = {
            "schema_version": 2,
            "instrument": ["A", "B"],
            "start": "2025-01-06",
            "end": "2025-01-10",
            "granularity": "Minute",
            "initial_cash": 1000000,
        }
    """))
    d = load_scenario(py)
    assert "instruments" in d
    assert d["instruments"] == ["A", "B"]
    assert "instrument" not in d


def test_normalize_scenario_idempotent():
    d = {
        "schema_version": 2,
        "instruments": ["A", "B"],
        "start": "2025-01-06",
        "end": "2025-01-10",
        "granularity": "Minute",
        "initial_cash": 1000000,
    }
    d2 = normalize_scenario(d)
    assert d2 is d  # 正規化済みはそのまま返る


def test_load_scenario_with_complex_suffix(tmp_path: Path):
    py = tmp_path / "foo.bar.py"
    py.write_text("# no SCENARIO")
    sidecar = tmp_path / "foo.bar.json"
    sidecar.write_text(json.dumps({
        "scenario": {
            "schema_version": 1,
            "instrument": "1301.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000,
        }
    }))
    d = load_scenario(py)
    assert d["instrument"] == "1301.TSE"
