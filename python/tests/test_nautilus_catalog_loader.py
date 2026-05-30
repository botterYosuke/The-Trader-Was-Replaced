"""
Tests for nautilus_catalog_loader.

Uses a monkeypatched fake ParquetDataCatalog so we don't need real parquet files on disk.
The real-catalog integration is left for a slow test once a sample catalog is checked in.
"""

from pathlib import Path

import pytest

from engine.core import DataEngine
from engine.nautilus_catalog_loader import load_bars, load_trades

# Real 8-byte (standard-precision) catalog snapshot taken from the shared Synology
# catalog (1301.TSE minute, first 5 rows). See GH #34 / Slice 0.
FIXTURE_STD_CATALOG = Path(__file__).parent / "fixtures" / "catalog_standard_precision"
FIXTURE_STD_BAR_TYPE = "1301.TSE-1-MINUTE-LAST-EXTERNAL"


# ---------------------------------------------------------------------------
# Fakes
# ---------------------------------------------------------------------------


class _Price:
    def __init__(self, v: float):
        self._v = v

    def as_double(self) -> float:
        return self._v


class _FakeBar:
    def __init__(self, close, ts_event_ns):
        self.open = _Price(close)
        self.high = _Price(close)
        self.low = _Price(close)
        self.close = _Price(close)
        self.volume = _Price(0.0)
        self.ts_event = ts_event_ns
        self.ts_init = 0


class _FakeTrade:
    def __init__(self, price, ts_event_ns):
        self.price = _Price(price)
        self.ts_event = ts_event_ns


class _FakeCatalog:
    """Records constructor args + query kwargs, returns a canned list."""

    instances: list = []

    def __init__(self, path, *args, **kwargs):
        self.path = path
        self.queries: list = []
        _FakeCatalog.instances.append(self)

    def query(self, **kwargs):
        self.queries.append(kwargs)
        return _FakeCatalog._next_return


@pytest.fixture(autouse=True)
def _reset_fake_catalog():
    _FakeCatalog.instances = []
    _FakeCatalog._next_return = []
    yield


@pytest.fixture
def patched_catalog(tmp_path, monkeypatch):
    """Monkeypatch ParquetDataCatalog inside the loader module."""
    import nautilus_trader.persistence.catalog as catalog_pkg

    monkeypatch.setattr(catalog_pkg, "ParquetDataCatalog", _FakeCatalog)
    # Ensure path exists so _resolve_catalog_path passes.
    catalog_dir = tmp_path / "catalog"
    catalog_dir.mkdir()
    return catalog_dir


# ---------------------------------------------------------------------------
# load_bars
# ---------------------------------------------------------------------------


def test_load_bars_resolves_path_and_queries_with_bar_class(patched_catalog):
    from nautilus_trader.model.data import Bar

    _FakeCatalog._next_return = [_FakeBar(100.0, 1_000_000_000)]

    result = load_bars(patched_catalog, instrument_ids=["AAPL.NASDAQ"], start=1, end=2)

    assert len(result) == 1
    assert len(_FakeCatalog.instances) == 1
    catalog = _FakeCatalog.instances[0]
    assert catalog.path == str(patched_catalog)
    assert catalog.queries[0]["data_cls"] is Bar
    assert catalog.queries[0]["identifiers"] == ["AAPL.NASDAQ"]
    assert catalog.queries[0]["start"] == 1
    assert catalog.queries[0]["end"] == 2


def test_load_bars_raises_when_path_missing(tmp_path):
    missing = tmp_path / "does-not-exist"
    with pytest.raises(FileNotFoundError):
        load_bars(missing)


# ---------------------------------------------------------------------------
# load_trades
# ---------------------------------------------------------------------------


def test_load_trades_queries_with_trade_tick_class(patched_catalog):
    from nautilus_trader.model.data import TradeTick

    _FakeCatalog._next_return = [_FakeTrade(50.0, 2_000_000_000)]

    result = load_trades(patched_catalog, instrument_ids=["AAPL.NASDAQ"])

    assert len(result) == 1
    catalog = _FakeCatalog.instances[0]
    assert catalog.queries[0]["data_cls"] is TradeTick
    assert catalog.queries[0]["identifiers"] == ["AAPL.NASDAQ"]


# ---------------------------------------------------------------------------
# loader → runner → DataEngine end-to-end (with fake catalog)
# ---------------------------------------------------------------------------



# ---------------------------------------------------------------------------
# Precision preflight hard-gate (GH #34)
#
# A standard-precision (8-byte) catalog read by a high-precision (16-byte) nautilus
# build makes nautilus abort the whole *process* inside catalog.query() — uncatchable
# from Python, surfaced to the UI only as "transport error". The preflight must detect
# the width mismatch and raise a typed error *before* query() is ever reached.
# ---------------------------------------------------------------------------


def test_preflight_raises_typed_error_on_width_mismatch():
    from engine.nautilus_catalog_loader import (
        CatalogPrecisionMismatchError,
        _assert_catalog_precision_compatible,
    )

    # 8-byte fixture, but running build claims 16-byte -> mismatch.
    with pytest.raises(CatalogPrecisionMismatchError):
        _assert_catalog_precision_compatible(
            str(FIXTURE_STD_CATALOG),
            "bar",
            [FIXTURE_STD_BAR_TYPE],
            expected_bytes=16,
        )


def test_preflight_passes_when_width_matches():
    from engine.nautilus_catalog_loader import _assert_catalog_precision_compatible

    # 8-byte fixture, running build is 8-byte -> compatible, no raise.
    _assert_catalog_precision_compatible(
        str(FIXTURE_STD_CATALOG),
        "bar",
        [FIXTURE_STD_BAR_TYPE],
        expected_bytes=8,
    )


def test_preflight_noop_when_no_parquet(tmp_path):
    from engine.nautilus_catalog_loader import _assert_catalog_precision_compatible

    # No data/bar dir -> cannot determine width -> pass through silently.
    _assert_catalog_precision_compatible(str(tmp_path), "bar", ["X"], expected_bytes=16)


def test_preflight_prefix_matches_bare_instrument_id():
    from engine.nautilus_catalog_loader import (
        CatalogPrecisionMismatchError,
        _assert_catalog_precision_compatible,
    )

    # nautilus query(identifiers=["1301.TSE"]) prefix-matches the bar_type dir
    # "1301.TSE-1-MINUTE-LAST-EXTERNAL". The gate must follow the same prefix
    # semantics, else a bare instrument id no-ops and an 8-byte file reaches query().
    with pytest.raises(CatalogPrecisionMismatchError):
        _assert_catalog_precision_compatible(
            str(FIXTURE_STD_CATALOG),
            "bar",
            ["1301.TSE"],  # bare instrument id, not the full bar_type dir name
            expected_bytes=16,
        )


def test_load_bars_gates_before_query_on_mismatch(monkeypatch):
    """The gate must fire BEFORE catalog.query(): query() on a mismatched width
    aborts the whole process (SIGABRT), which Python cannot catch."""
    import engine.nautilus_catalog_loader as loader
    from engine.nautilus_catalog_loader import CatalogPrecisionMismatchError

    # Force a mismatch regardless of which precision build is installed.
    monkeypatch.setattr(loader, "_running_precision_bytes", lambda: 16)

    # If the gate regresses, query() is reached -> fail loudly but *safely*
    # (without touching the real abort-prone decoder).
    import nautilus_trader.persistence.catalog as catalog_pkg

    class _ExplodingCatalog:
        def __init__(self, *a, **k):
            pass

        def query(self, **k):
            raise AssertionError("query() reached — precision gate did not fire")

    monkeypatch.setattr(catalog_pkg, "ParquetDataCatalog", _ExplodingCatalog)

    with pytest.raises(CatalogPrecisionMismatchError):
        loader.load_bars(FIXTURE_STD_CATALOG, instrument_ids=[FIXTURE_STD_BAR_TYPE])


# ---------------------------------------------------------------------------
# Slow round-trip against a real ParquetDataCatalog
# ---------------------------------------------------------------------------


