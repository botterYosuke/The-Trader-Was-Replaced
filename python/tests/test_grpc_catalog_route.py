"""
gRPC LoadReplayData → catalog_path → NautilusBarsReplayProvider end-to-end.

Slow because it writes a real ParquetDataCatalog to disk and goes through gRPC.
"""

from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


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

    assert stub.StartEngine(
        engine_pb2.StartEngineRequest(request_id="r2", token=token)
    ).current_state == engine_pb2.RUNNING

    assert stub.PauseReplay(
        engine_pb2.PauseReplayRequest(request_id="r3", token=token)
    ).current_state == engine_pb2.PAUSED

    # Two steps → third bar.
    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="r4", token=token))
    stub.StepReplay(engine_pb2.StepReplayRequest(request_id="r5", token=token))

    state = engine.get_current_state()
    assert state.timestamp_ms == 15_000
    assert state.price == 110.25


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
