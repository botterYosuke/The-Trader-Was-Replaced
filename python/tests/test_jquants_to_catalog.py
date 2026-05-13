"""
Tests for J-Quants CSV → Nautilus Catalog conversion.
"""

import os
from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.jquants_to_catalog import (
    convert_daily_to_catalog,
    convert_minute_to_catalog,
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

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            instrument_ids=[bar_type_str],
            granularity=engine_pb2.DAILY,
            catalog_path=str(catalog_dir),
            token=token,
        )
    )
    assert load_resp.success, load_resp.error_message
    # Prime → first bar's open is the known 3284.0.
    assert engine.get_current_state().price == 3284.0

    assert stub.StartEngine(
        engine_pb2.StartEngineRequest(request_id="r2", token=token)
    ).current_state == engine_pb2.RUNNING
    assert stub.PauseReplay(
        engine_pb2.PauseReplayRequest(request_id="r3", token=token)
    ).current_state == engine_pb2.PAUSED

    # Step once → second bar (close value matches the existing jquants integration test: 3333.0)
    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="r4", token=token))
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

    load_resp = stub.LoadReplayData(
        engine_pb2.LoadReplayDataRequest(
            request_id="r1",
            instrument_ids=[bar_type_str],
            granularity=engine_pb2.MINUTE,
            catalog_path=str(catalog_dir),
            token=token,
        )
    )
    assert load_resp.success, load_resp.error_message
    # Prime → first bar's close is 3308.0
    assert engine.get_current_state().price == 3308.0

    assert stub.StartEngine(
        engine_pb2.StartEngineRequest(request_id="r2", token=token)
    ).current_state == engine_pb2.RUNNING
    assert stub.PauseReplay(
        engine_pb2.PauseReplayRequest(request_id="r3", token=token)
    ).current_state == engine_pb2.PAUSED

    # Step once → second bar closes at 3301.0
    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="r4", token=token))
    assert engine.get_current_state().price == 3301.0
