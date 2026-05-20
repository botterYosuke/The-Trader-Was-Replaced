"""
Tests for nautilus_catalog_loader.

Uses a monkeypatched fake ParquetDataCatalog so we don't need real parquet files on disk.
The real-catalog integration is left for a slow test once a sample catalog is checked in.
"""

import pytest

from engine.core import DataEngine
from engine.nautilus_catalog_loader import load_bars, load_trades
from engine.nautilus_runner import NautilusReplayRunner
from engine.replay import BaseReplayProvider


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


class _OneTickProvider(BaseReplayProvider):
    def __init__(self):
        self._done = False

    def get_next_tick(self):
        if self._done:
            return None
        self._done = True
        return (0.001, 1.0, 1.0, 1.0, 1.0)

    def is_exhausted(self) -> bool:
        return self._done


def test_load_bars_feeds_runner_and_updates_engine_state(patched_catalog):
    _FakeCatalog._next_return = [
        _FakeBar(100.0, 5_000_000_000),
        _FakeBar(110.0, 10_000_000_000),
    ]

    engine = DataEngine(replay_provider=_OneTickProvider())
    runner = NautilusReplayRunner(engine)

    runner.run_bars(load_bars(patched_catalog, instrument_ids=["X"]))

    state = engine.get_current_state()
    assert state.timestamp_ms == 10_000
    assert state.price == 110.0


# ---------------------------------------------------------------------------
# Slow round-trip against a real ParquetDataCatalog
# ---------------------------------------------------------------------------


@pytest.mark.slow
def test_real_catalog_round_trip_bars_to_engine_state(tmp_path):
    """
    Write real Bars into a ParquetDataCatalog on disk, then read them back through
    load_bars → NautilusReplayRunner → DataEngine and verify the final state.

    Confirms:
      - identifiers filtering with a BarType string works
      - returned bars are in ts_event order
      - the adapter's ns → ms conversion lines up with what we wrote
    """
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path.resolve()))

    bar_type = BarType.from_str("AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL")

    def _bar(close: str, ts_event_ns: int) -> Bar:
        # high >= low/close, low <= close — keep all four equal to satisfy validation.
        p = Price.from_str(close)
        return Bar(
            bar_type=bar_type,
            open=p,
            high=p,
            low=p,
            close=p,
            volume=Quantity.from_int(1000),
            ts_event=ts_event_ns,
            ts_init=ts_event_ns,
        )

    bars_in = [
        _bar("100.50", 5_000_000_000),    # 5_000 ms
        _bar("101.25", 10_000_000_000),   # 10_000 ms
        _bar("102.00", 15_000_000_000),   # 15_000 ms
    ]
    catalog.write_data(bars_in)

    loaded = load_bars(catalog_path, instrument_ids=[str(bar_type)])
    assert len(loaded) == 3
    assert [int(b.ts_event) for b in loaded] == [5_000_000_000, 10_000_000_000, 15_000_000_000]

    engine = DataEngine(replay_provider=_OneTickProvider())
    runner = NautilusReplayRunner(engine)
    runner.run_bars(loaded)

    state = engine.get_current_state()
    assert state.timestamp_ms == 15_000
    assert state.close == 102.0
    assert state.price == 102.0
