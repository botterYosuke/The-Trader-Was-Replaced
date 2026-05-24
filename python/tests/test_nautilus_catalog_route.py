"""
Tests for the Nautilus catalog route in DataEngine.load_replay_data.

Verifies:
  - DataEngine(nautilus_catalog_path=...) wires NautilusBarsReplayProvider
  - granularity="Daily" / "Minute" are accepted, "Trade" is rejected
  - LoadReplayData → StartEngine → PauseReplay → StepReplay advances state
"""

import pytest

from engine.core import DataEngine


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
        self.ts_init = ts_event_ns


@pytest.fixture
def patched_load_bars(tmp_path, monkeypatch):
    """Patch load_bars where NautilusBarsReplayProvider imports it."""
    catalog_dir = tmp_path / "catalog"
    catalog_dir.mkdir()

    calls = {}

    def _fake_load_bars(catalog_path, instrument_ids=None, start=None, end=None):
        calls["catalog_path"] = str(catalog_path)
        calls["instrument_ids"] = instrument_ids
        calls["start"] = start
        calls["end"] = end
        return [
            _FakeBar(100.0, 5_000_000_000),
            _FakeBar(110.0, 10_000_000_000),
            _FakeBar(120.0, 15_000_000_000),
        ]

    import engine.nautilus_catalog_loader as loader_mod

    monkeypatch.setattr(loader_mod, "load_bars", _fake_load_bars)
    return catalog_dir, calls


# ---------------------------------------------------------------------------
# Route wiring
# ---------------------------------------------------------------------------


def test_catalog_route_loads_and_primes_first_bar(patched_load_bars):
    catalog_dir, calls = patched_load_bars
    engine = DataEngine(nautilus_catalog_path=str(catalog_dir))

    # D17: pass instrument_id ("AAPL.NASDAQ"), not BarType string
    success, error = engine.load_replay_data(
        instrument_ids=["AAPL.NASDAQ"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Minute",
    )

    assert success, error
    assert engine.replay_state == "LOADED"
    state = engine.get_current_state()
    # First bar primes the state: close=100.0 at ts_event=5_000_000_000 ns → 5_000 ms
    assert state.price == 100.0
    assert state.timestamp_ms == 5_000

    # D17: load_bars is called with the converted BarType string
    assert calls["instrument_ids"] == ["AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"]
    assert calls["start"] == "2024-07-01"
    assert calls["end"] == "2024-07-02"


def test_precision_mismatch_does_not_trigger_catalog_write(tmp_path, monkeypatch):
    """GH #34 (Critical): a CatalogPrecisionMismatchError during catalog read must NOT
    fall through to ensure_jquants_catalog.

    On a high-precision build that fallback would WRITE high-precision data into the
    shared standard-precision catalog and corrupt it for the Windows machines — the one
    thing the issue forbids. The mismatch must propagate as a typed error, untouched.
    """
    from engine.nautilus_catalog_loader import CatalogPrecisionMismatchError
    import engine.nautilus_catalog_loader as loader_mod
    import engine.core as core_mod

    catalog_dir = tmp_path / "catalog"
    catalog_dir.mkdir()

    def _raise_mismatch(*a, **k):
        raise CatalogPrecisionMismatchError(
            "Catalog precision mismatch: stores fixed_size_binary[8] ... PRECISION_BYTES=16"
        )

    monkeypatch.setattr(loader_mod, "load_bars", _raise_mismatch)

    ensure_calls: list = []
    monkeypatch.setattr(
        core_mod, "ensure_jquants_catalog", lambda *a, **k: ensure_calls.append((a, k))
    )

    class _FakeLoader:
        base_dir = str(tmp_path / "jq")

    # nautilus_catalog_path + a jquants loader → the fallback CONDITION is satisfied.
    engine = DataEngine(nautilus_catalog_path=str(catalog_dir), jquants_loader=_FakeLoader())

    with pytest.raises(CatalogPrecisionMismatchError):
        engine.load_replay_data(
            instrument_ids=["1301.TSE"],
            start_date="2025-01-06",
            end_date="2025-01-10",
            granularity="Minute",
        )

    assert ensure_calls == [], "shared catalog write attempted on precision mismatch"
    assert engine.replay_state == "IDLE"


def test_catalog_route_rejects_trade_granularity(patched_load_bars):
    catalog_dir, _ = patched_load_bars
    engine = DataEngine(nautilus_catalog_path=str(catalog_dir))

    # D17: pass instrument_id
    success, error = engine.load_replay_data(
        instrument_ids=["AAPL.NASDAQ"],
        granularity="Trade",
    )

    assert not success
    assert "nautilus catalog" in (error or "")
    assert engine.replay_state == "IDLE"


def test_catalog_route_step_advances_through_bars(patched_load_bars):
    catalog_dir, _ = patched_load_bars
    engine = DataEngine(nautilus_catalog_path=str(catalog_dir))

    # D17: pass instrument_id
    assert engine.load_replay_data(
        instrument_ids=["AAPL.NASDAQ"],
        granularity="Minute",
    )[0]
    assert engine.start_engine()[0]
    assert engine.pause_replay()[0]

    # Step to second bar.
    assert engine.step_replay()[0]
    state = engine.get_current_state()
    assert state.timestamp_ms == 10_000
    assert state.price == 110.0

    # Step to third bar.
    assert engine.step_replay()[0]
    state = engine.get_current_state()
    assert state.timestamp_ms == 15_000
    assert state.price == 120.0


def test_catalog_route_propagates_no_data_error(tmp_path, monkeypatch):
    catalog_dir = tmp_path / "catalog"
    catalog_dir.mkdir()

    import engine.nautilus_catalog_loader as loader_mod

    monkeypatch.setattr(loader_mod, "load_bars", lambda *a, **kw: [])

    engine = DataEngine(nautilus_catalog_path=str(catalog_dir))
    # D17: pass instrument_id
    success, error = engine.load_replay_data(
        instrument_ids=["AAPL.NASDAQ"],
        granularity="Minute",
    )

    assert not success
    assert "No nautilus catalog bars" in (error or "")


def test_catalog_route_takes_priority_over_jquants_loader(patched_load_bars):
    """When both nautilus_catalog_path and jquants_loader are configured, catalog wins."""
    catalog_dir, _ = patched_load_bars

    class _SentinelLoader:
        def load_daily_rows(self, *a, **kw):
            raise AssertionError("jquants route should not be used when catalog is configured")

        def load_minute_rows(self, *a, **kw):
            raise AssertionError("jquants route should not be used when catalog is configured")

    engine = DataEngine(
        nautilus_catalog_path=str(catalog_dir),
        jquants_loader=_SentinelLoader(),
    )

    # D17: pass instrument_id
    success, error = engine.load_replay_data(
        instrument_ids=["AAPL.NASDAQ"],
        granularity="Minute",
    )
    assert success, error


# ---------------------------------------------------------------------------
# Slow round-trip: real catalog → DataEngine state
# ---------------------------------------------------------------------------


@pytest.mark.slow
def test_real_catalog_route_round_trip(tmp_path):
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path.resolve()))

    bar_type = BarType.from_str("AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL")

    def _bar(close: str, ts_event_ns: int) -> Bar:
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

    catalog.write_data([
        _bar("100.50", 5_000_000_000),
        _bar("105.00", 10_000_000_000),
        _bar("110.25", 15_000_000_000),
    ])

    engine = DataEngine(nautilus_catalog_path=str(catalog_path))
    # D17: pass instrument_id ("AAPL.NASDAQ"), not bar_type string
    success, error = engine.load_replay_data(
        instrument_ids=["AAPL.NASDAQ"],
        granularity="Minute",
    )
    assert success, error

    # Priming should put us on the first bar.
    state = engine.get_current_state()
    assert state.timestamp_ms == 5_000
    assert state.price == 100.5

    assert engine.start_engine()[0]
    assert engine.pause_replay()[0]
    assert engine.step_replay()[0]
    assert engine.step_replay()[0]

    state = engine.get_current_state()
    assert state.timestamp_ms == 15_000
    assert state.price == 110.25
