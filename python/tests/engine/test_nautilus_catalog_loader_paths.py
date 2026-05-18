"""Path-handling tests for nautilus_catalog_loader and jquants_to_catalog.

These tests guard two regressions:
  1. `_resolve_catalog_path` and `_write_bars_to_catalog` must never call
     `pathlib.Path.resolve()` -- on Windows it walks reparse points and
     rewrites mapped drives back to their UNC form (which DataFusion cannot
     handle).
  2. UNC paths (both `//host/...` and `\\host\...`) must be rejected with a
     `ValueError` *before* any disk write occurs.
"""

from __future__ import annotations

import pathlib
from pathlib import Path

import pytest

from engine.nautilus_catalog_loader import _resolve_catalog_path
from engine.jquants_to_catalog import _write_bars_to_catalog


# ---------------------------------------------------------------------------
# read side: _resolve_catalog_path
# ---------------------------------------------------------------------------


def _explode_resolve(self, *args, **kwargs):  # noqa: ANN001
    raise RuntimeError("Path.resolve must not be called")


def test_resolve_does_not_call_pathlib_resolve(tmp_path, monkeypatch):
    catalog_dir = tmp_path / "catalog"
    catalog_dir.mkdir()
    monkeypatch.setattr(pathlib.Path, "resolve", _explode_resolve)

    result = _resolve_catalog_path(catalog_dir)

    assert Path(result).is_absolute()
    assert str(tmp_path) in result


def test_resolve_rejects_unc_forward_slashes():
    with pytest.raises(ValueError, match="UNC"):
        _resolve_catalog_path("//sasaco-ds218/share/cat")


def test_resolve_rejects_unc_backslashes():
    with pytest.raises(ValueError, match="UNC"):
        _resolve_catalog_path(r"\\sasaco-ds218\share\cat")


def test_resolve_accepts_relative_and_existing(tmp_path, monkeypatch):
    (tmp_path / "catalog").mkdir()
    monkeypatch.chdir(tmp_path)

    result = _resolve_catalog_path("catalog")

    assert Path(result).is_absolute()
    assert str(tmp_path) in result


# ---------------------------------------------------------------------------
# write side: _write_bars_to_catalog
# ---------------------------------------------------------------------------


def test_write_bars_does_not_call_pathlib_resolve(tmp_path, monkeypatch):
    monkeypatch.setattr(pathlib.Path, "resolve", _explode_resolve)

    result = _write_bars_to_catalog(
        rows=[],
        ticks_fn=lambda rows: [],
        bar_type_str="AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL",
        catalog_path=tmp_path / "cat",
        price_precision=2,
    )

    assert result.rows_written == 0
    assert Path(result.catalog_path).is_absolute()
    assert not result.catalog_path.startswith("\\\\")
    assert not result.catalog_path.startswith("//")


def test_write_bars_rejects_unc_forward_slashes(tmp_path):
    sentinel = tmp_path / "sentinel"
    sentinel.mkdir()

    with pytest.raises(ValueError, match="UNC"):
        _write_bars_to_catalog(
            rows=[],
            ticks_fn=lambda rows: [],
            bar_type_str="AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL",
            catalog_path=Path("//host/share/cat"),
            price_precision=2,
        )

    assert list(sentinel.iterdir()) == []


def test_write_bars_rejects_unc_backslashes(tmp_path):
    sentinel = tmp_path / "sentinel"
    sentinel.mkdir()

    with pytest.raises(ValueError, match="UNC"):
        _write_bars_to_catalog(
            rows=[],
            ticks_fn=lambda rows: [],
            bar_type_str="AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL",
            catalog_path=Path(r"\\host\share\cat"),
            price_precision=2,
        )

    assert list(sentinel.iterdir()) == []
