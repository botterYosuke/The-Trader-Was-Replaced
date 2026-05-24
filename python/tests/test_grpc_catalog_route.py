"""
gRPC LoadReplayData → catalog_path → NautilusBarsReplayProvider end-to-end.

Slow because it writes a real ParquetDataCatalog to disk and goes through gRPC.
"""

from concurrent import futures
from pathlib import Path

import grpc
import pytest

from engine.core import DataEngine
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer

# Real 8-byte (standard-precision) catalog snapshot — see GH #34 / Slice 0.
FIXTURE_STD_CATALOG = Path(__file__).parent / "fixtures" / "catalog_standard_precision"


@pytest.fixture
def catalog_grpc_server():
    """gRPC server with a DataEngine that has no preconfigured catalog path.
    The catalog path is supplied per-request via LoadReplayDataRequest.catalog_path."""
    token = "test-token"
    engine = DataEngine()

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()
    yield (port, token, engine)
    server.stop(0)


_INSTRUMENT_ID = "AAPL.NASDAQ"  # D17: instrument id (not bar_type)


def _write_sample_catalog(catalog_path):
    """Write 3 bars into a fresh ParquetDataCatalog using instrument_id format."""
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    catalog = ParquetDataCatalog(str(catalog_path.resolve()))
    # D17: bar_type = instrument_id + granularity spec
    bar_type = BarType.from_str(f"{_INSTRUMENT_ID}-1-MINUTE-LAST-EXTERNAL")

    def _bar(close: str, ts_event_ns: int) -> Bar:
        p = Price.from_str(close)
        return Bar(
            bar_type=bar_type,
            open=p, high=p, low=p, close=p,
            volume=Quantity.from_int(1000),
            ts_event=ts_event_ns,
            ts_init=ts_event_ns,
        )

    catalog.write_data([
        _bar("100.50", 5_000_000_000),
        _bar("105.00", 10_000_000_000),
        _bar("110.25", 15_000_000_000),
    ])


@pytest.mark.slow
def test_grpc_load_replay_data_with_catalog_path_then_step(catalog_grpc_server, tmp_path):
    port, token, engine = catalog_grpc_server
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    _write_sample_catalog(catalog_path)

    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            # D17: pass instrument_id ("AAPL.NASDAQ"), not bar_type string
            instrument_ids=[_INSTRUMENT_ID],
            granularity=engine_pb2.MINUTE,
            catalog_path=str(catalog_path),
            token=token,
        )
    )
    assert load_resp.success, load_resp.error_message
    assert load_resp.current_state == engine_pb2.LOADED
    # Prime gives us the first bar.
    state = engine.get_current_state()
    assert state.timestamp_ms == 5_000
    assert state.price == 100.5

    # Manual stepping is an engine-level capability: the gRPC StartEngine RPC now
    # requires a strategy_file and runs it to completion (see test_grpc_control.py),
    # so step-able RUNNING is driven via the engine object directly (cf.
    # test_jquants_to_catalog::test_ensure_jquants_catalog_replayed_via_engine).
    engine.start_engine()
    engine.pause_replay()

    # Two steps → third bar.
    engine.step_replay()
    engine.step_replay()

    state = engine.get_current_state()
    assert state.timestamp_ms == 15_000
    assert state.price == 110.25


def test_grpc_load_replay_data_precision_mismatch_surfaces_typed_error(
    catalog_grpc_server, monkeypatch
):
    """GH #34: a width-mismatched catalog must surface a typed error through gRPC,
    not abort the backend (which the UI only ever sees as "transport error").

    Build-independent: we force the running precision to 16-byte against the real
    8-byte fixture, so the preflight fires regardless of which wheel is installed.
    The gRPC server runs in this process, so patching the loader module reaches it.
    """
    import engine.nautilus_catalog_loader as loader

    monkeypatch.setattr(loader, "_running_precision_bytes", lambda: 16)

    port, token, _ = catalog_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            instrument_ids=["1301.TSE"],
            granularity=engine_pb2.MINUTE,
            catalog_path=str(FIXTURE_STD_CATALOG),
            token=token,
        )
    )

    # Backend stayed up and reported the real cause (no SIGABRT, no transport error).
    assert not resp.success
    assert resp.error_code == "CATALOG_PRECISION_MISMATCH"
    assert "precision mismatch" in resp.error_message.lower()
    assert "PRECISION_BYTES=16" in resp.error_message


def test_load_replay_data_missing_symbol_does_not_write_into_shared_catalog(monkeypatch):
    """GH #34 High: a 16-byte build pointed at an existing 8-byte shared catalog,
    requesting a symbol that has NO parquet dir, must NOT call ensure_jquants_catalog.

    The identifier-scoped read-guard no-ops on a missing symbol (files == []),
    the provider then raises "No bars" ValueError, and core.py's fallback writes
    16-byte bars into the 8-byte shared catalog — corrupting it for Windows.
    """
    import engine.nautilus_catalog_loader as loader
    import engine.core as core_mod

    # Force a 16-byte running build so the mismatch reproduces on any wheel.
    monkeypatch.setattr(loader, "_running_precision_bytes", lambda: 16)

    # Spy: the corrupting write must never be reached.
    called = {"n": 0}
    def _spy_ensure(*a, **k):
        called["n"] += 1
        raise AssertionError(
            "ensure_jquants_catalog must NOT be called: it would write 16-byte "
            "bars into the 8-byte shared catalog (GH #34)"
        )
    monkeypatch.setattr(core_mod, "ensure_jquants_catalog", _spy_ensure)

    # The jquants fallback branch requires jquants_loader_base_dir + start/end.
    # jquants_loader_base_dir is a property over self._jquants_loader.base_dir,
    # so inject a tiny fake loader carrying a truthy base_dir.
    class _FakeLoader:
        base_dir = "/tmp/jquants-unused"
    engine = DataEngine(
        nautilus_catalog_path=str(FIXTURE_STD_CATALOG),
        jquants_loader=_FakeLoader(),
    )

    # Request a symbol that does NOT exist in the 8-byte fixture
    # (so the read-guard no-ops and the fallback write path is exercised).
    with pytest.raises(loader.CatalogPrecisionMismatchError) as excinfo:
        engine.load_replay_data(
            instrument_ids=["9999.TSE"],
            granularity="Minute",
            start_date="2024-01-01",
            end_date="2024-01-31",
            catalog_path=str(FIXTURE_STD_CATALOG),
        )

    # The corrupting write was never reached, and the typed error propagated
    # so the gRPC layer can map it to CATALOG_PRECISION_MISMATCH.
    assert called["n"] == 0
    assert "precision" in str(excinfo.value).lower()


def test_load_replay_data_direct_jquants_route_does_not_write_into_shared_catalog(monkeypatch):
    """GH #34 (codex round-2 High): the DIRECT J-Quants route — where
    load_replay_data is called with NO catalog_path arg and the engine has a
    configured jquants_catalog_path — must also guard the catalog-wide precision
    BEFORE writing.

    A 16-byte build pointed at an existing 8-byte shared jquants_catalog_path must
    raise CatalogPrecisionMismatchError and must NOT call ensure_jquants_catalog
    (which would corrupt the 8-byte catalog for Windows). The typed error must
    propagate so gRPC maps it to CATALOG_PRECISION_MISMATCH.
    """
    import engine.nautilus_catalog_loader as loader
    import engine.core as core_mod

    # Force a 16-byte running build so the mismatch reproduces on any wheel.
    monkeypatch.setattr(loader, "_running_precision_bytes", lambda: 16)

    # Spy: the corrupting write must never be reached.
    called = {"n": 0}
    def _spy_ensure(*a, **k):
        called["n"] += 1
        raise AssertionError(
            "ensure_jquants_catalog must NOT be called on the direct J-Quants "
            "route: it would write 16-byte bars into the 8-byte shared catalog "
            "(GH #34 codex round-2)"
        )
    monkeypatch.setattr(core_mod, "ensure_jquants_catalog", _spy_ensure)

    # Enter the DIRECT jquants branch: NO nautilus_catalog_path, NO catalog_path
    # arg → effective_catalog_path is None → falls through to the
    # `self._jquants_loader is not None` block, which writes to
    # self._jquants_catalog_path. The fixture is the real 8-byte catalog.
    class _FakeLoader:
        base_dir = "/tmp/jquants-unused"
    engine = DataEngine(
        jquants_loader=_FakeLoader(),
        jquants_catalog_path=str(FIXTURE_STD_CATALOG),
    )

    with pytest.raises(loader.CatalogPrecisionMismatchError) as excinfo:
        engine.load_replay_data(
            instrument_ids=["9999.TSE"],
            granularity="Minute",
            start_date="2024-01-01",
            end_date="2024-01-31",
            # catalog_path intentionally omitted → routes to the direct jquants branch
        )

    assert called["n"] == 0
    assert "precision" in str(excinfo.value).lower()


def test_grpc_load_replay_data_without_catalog_path_field(catalog_grpc_server):
    """When catalog_path is not set, the request should not route to the catalog provider."""
    port, token, _ = catalog_grpc_server
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            # D17: instrument_id format (not bar_type string)
            instrument_ids=[_INSTRUMENT_ID],
            granularity=engine_pb2.MINUTE,
            token=token,
            # catalog_path intentionally omitted
        )
    )
    # No catalog, no jquants → falls through to "not configured"
    assert not resp.success
    assert "not configured" in resp.error_message
