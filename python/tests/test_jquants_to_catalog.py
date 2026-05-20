"""
Tests for J-Quants CSV → Nautilus Catalog conversion.
"""

import os
from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.jquants_to_catalog import (
    JQuantsCatalogResult,
    convert_daily_to_catalog,
    convert_minute_to_catalog,
    ensure_jquants_catalog,
    instrument_id_to_bar_type,
)
from engine.nautilus_catalog_loader import load_bars
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer

DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")


# ---------------------------------------------------------------------------
# instrument_id_to_bar_type — fast pure-mapping
# ---------------------------------------------------------------------------


def test_instrument_id_to_bar_type_daily():
    assert instrument_id_to_bar_type("7203.TSE", "Daily") == "7203.TSE-1-DAY-LAST-EXTERNAL"


def test_instrument_id_to_bar_type_minute():
    assert instrument_id_to_bar_type("7203.TSE", "Minute") == "7203.TSE-1-MINUTE-LAST-EXTERNAL"


def test_instrument_id_to_bar_type_rejects_trade():
    with pytest.raises(ValueError, match="Unsupported granularity"):
        instrument_id_to_bar_type("7203.TSE", "Trade")


# ---------------------------------------------------------------------------
# Slow: round-trip through real J-Quants CSV → catalog → load_bars
# ---------------------------------------------------------------------------


@pytest.mark.slow
def test_convert_daily_to_catalog_roundtrip_load_bars(tmp_path):
    catalog_dir = tmp_path / "catalog"
    bar_type_str = convert_daily_to_catalog(
        base_dir=DATA_DIR,
        catalog_path=catalog_dir,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
    )

    assert bar_type_str == "7203.TSE-1-DAY-LAST-EXTERNAL"

    loaded = load_bars(catalog_dir, instrument_ids=[bar_type_str])
    assert len(loaded) >= 1
    # First J-Quants row for 7203.TSE on 2024-07-01 closes at 3284.0
    # (verified earlier via test_data_engine_load_daily_jquants_primes_and_steps which
    # checks state.price — state.price is the reducer's close.)
    assert loaded[0].close.as_double() == 3284.0


# ---------------------------------------------------------------------------
# Slow: end-to-end through gRPC LoadReplayData → StepReplay
# ---------------------------------------------------------------------------


@pytest.fixture
def grpc_server_empty_engine():
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


@pytest.mark.slow
def test_jquants_csv_converted_then_replayed_via_grpc(grpc_server_empty_engine, tmp_path):
    catalog_dir = tmp_path / "catalog"
    bar_type_str = convert_daily_to_catalog(
        base_dir=DATA_DIR,
        catalog_path=catalog_dir,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
    )

    port, token, engine = grpc_server_empty_engine
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    # D17: pass instrument_id ("7203.TSE"), not bar_type string
    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            instrument_ids=["7203.TSE"],
            granularity=engine_pb2.DAILY,
            catalog_path=str(catalog_dir),
            token=token,
        )
    )
    assert load_resp.success, load_resp.error_message
    # Prime → first bar's open is the known 3284.0.
    assert engine.get_current_state().price == 3284.0

    # Manual stepping is an engine-level capability (the gRPC StartEngine RPC now
    # requires a strategy_file and runs to completion). Drive start/pause/step via
    # the engine object directly, mirroring test_ensure_jquants_catalog_replayed_via_engine.
    engine.start_engine()
    engine.pause_replay()

    # Step once → second bar (close value matches the existing jquants integration test: 3333.0)
    engine.step_replay()
    assert engine.get_current_state().price == 3333.0


# ---------------------------------------------------------------------------
# Slow: Minute round-trip through real J-Quants CSV → catalog → load_bars
# ---------------------------------------------------------------------------


@pytest.mark.slow
def test_convert_minute_to_catalog_roundtrip_load_bars(tmp_path):
    catalog_dir = tmp_path / "catalog"
    bar_type_str = convert_minute_to_catalog(
        base_dir=DATA_DIR,
        catalog_path=catalog_dir,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-01",
    )

    assert bar_type_str == "7203.TSE-1-MINUTE-LAST-EXTERNAL"

    loaded = load_bars(catalog_dir, instrument_ids=[bar_type_str])
    assert len(loaded) >= 1
    # First minute bar for 7203.TSE on 2024-07-01 09:00 closes at 3308.0
    assert loaded[0].close.as_double() == 3308.0


@pytest.mark.slow
def test_jquants_minute_csv_converted_then_replayed_via_grpc(
    grpc_server_empty_engine, tmp_path
):
    catalog_dir = tmp_path / "catalog"
    bar_type_str = convert_minute_to_catalog(
        base_dir=DATA_DIR,
        catalog_path=catalog_dir,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-01",
    )

    port, token, engine = grpc_server_empty_engine
    channel = grpc.insecure_channel(f"localhost:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)

    # D17: pass instrument_id ("7203.TSE"), not bar_type string
    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            instrument_ids=["7203.TSE"],
            granularity=engine_pb2.MINUTE,
            catalog_path=str(catalog_dir),
            token=token,
        )
    )
    assert load_resp.success, load_resp.error_message
    # Prime → first bar's close is 3308.0
    assert engine.get_current_state().price == 3308.0

    # Manual stepping is an engine-level capability (the gRPC StartEngine RPC now
    # requires a strategy_file and runs to completion). Drive start/pause/step via
    # the engine object directly, mirroring test_ensure_jquants_catalog_replayed_via_engine.
    engine.start_engine()
    engine.pause_replay()

    # Step once → second bar closes at 3301.0
    engine.step_replay()
    assert engine.get_current_state().price == 3301.0


# ---------------------------------------------------------------------------
# ensure_jquants_catalog — fast pure-logic tests (no nautilus, no catalog I/O)
# ---------------------------------------------------------------------------


def test_ensure_jquants_catalog_rejects_trade_granularity(tmp_path):
    with pytest.raises(ValueError, match="Unsupported granularity"):
        ensure_jquants_catalog(
            base_dir=DATA_DIR,
            catalog_path=tmp_path / "catalog",
            instrument_id="7203.TSE",
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity="Trade",
        )


def test_ensure_jquants_catalog_rejects_missing_data(tmp_path):
    with pytest.raises(ValueError, match="No daily rows"):
        ensure_jquants_catalog(
            base_dir=str(tmp_path),  # empty dir → no CSV files
            catalog_path=tmp_path / "catalog",
            instrument_id="7203.TSE",
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity="Daily",
        )


# ---------------------------------------------------------------------------
# Slow: ensure_jquants_catalog round-trips (Daily + Minute)
# ---------------------------------------------------------------------------


@pytest.mark.slow
def test_ensure_jquants_catalog_daily_returns_result(tmp_path):
    result = ensure_jquants_catalog(
        base_dir=DATA_DIR,
        catalog_path=tmp_path / "catalog",
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )

    assert isinstance(result, JQuantsCatalogResult)
    assert result.bar_type == "7203.TSE-1-DAY-LAST-EXTERNAL"
    assert result.rows_written == 2
    assert result.catalog_path  # non-empty resolved path

    # catalog_path in result is ready to pass to load_replay_data
    loaded = load_bars(result.catalog_path, instrument_ids=[result.bar_type])
    assert loaded[0].close.as_double() == 3284.0


@pytest.mark.slow
def test_ensure_jquants_catalog_minute_returns_result(tmp_path):
    result = ensure_jquants_catalog(
        base_dir=DATA_DIR,
        catalog_path=tmp_path / "catalog",
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Minute",
    )

    assert isinstance(result, JQuantsCatalogResult)
    assert result.bar_type == "7203.TSE-1-MINUTE-LAST-EXTERNAL"
    assert result.rows_written >= 1
    assert result.catalog_path

    loaded = load_bars(result.catalog_path, instrument_ids=[result.bar_type])
    assert loaded[0].close.as_double() == 3308.0


@pytest.mark.slow
def test_ensure_jquants_catalog_replayed_via_engine(tmp_path):
    """ensure_jquants_catalog result plugs directly into DataEngine.load_replay_data."""
    result = ensure_jquants_catalog(
        base_dir=DATA_DIR,
        catalog_path=tmp_path / "catalog",
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )

    engine = DataEngine()
    # D17: pass instrument_id ("7203.TSE"), not bar_type string
    ok, err = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        granularity="Daily",
        catalog_path=result.catalog_path,
    )
    assert ok, err
    assert engine.get_current_state().price == 3284.0

    engine.start_engine()
    engine.pause_replay()
    engine.step_replay()
    assert engine.get_current_state().price == 3333.0
