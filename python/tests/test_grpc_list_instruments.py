"""
gRPC ListInstruments — unit and integration tests.
"""

from concurrent import futures
from pathlib import Path

import grpc
import pytest

from engine.core import DataEngine
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer

TOKEN = "test-token"


@pytest.fixture
def grpc_server():
    engine = DataEngine()
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    yield port, engine
    server.stop(0)


def _make_stub(port: int):
    channel = grpc.insecure_channel(f"localhost:{port}")
    return engine_pb2_grpc.DataEngineStub(channel)


# ---------------------------------------------------------------------------
# No catalog configured
# ---------------------------------------------------------------------------

def test_list_instruments_no_catalog(grpc_server):
    port, _ = grpc_server
    stub = _make_stub(port)
    resp = stub.ListInstruments(
        engine_pb2.ListInstrumentsRequest(token=TOKEN)
    )
    assert not resp.success
    assert resp.error_message  # some message, no crash


# ---------------------------------------------------------------------------
# With a fake on-disk catalog (no Nautilus dependency) — fast path
# ---------------------------------------------------------------------------

def test_list_instruments_returns_structured_instruments(tmp_path):
    """
    Phase 8 §3.5: ListInstrumentsResponse must populate `instruments`
    (id + name + market) in addition to the legacy `instrument_ids`.

    For Replay catalog sources the venue/market metadata is unknown, so
    `name` falls back to `id` and `market` is empty. Live venue adapters
    are expected to fill these fields when they back ListInstruments in
    a later phase.
    """
    catalog_path = tmp_path / "catalog"
    bar_dir = catalog_path / "data" / "bar"
    bar_dir.mkdir(parents=True)
    # Catalog dir names follow the pattern `<id>-<n>-<RES>` where <id>
    # contains the instrument id (e.g. "1301.TSE"). The backend extracts
    # the id by regex `^(.+?)-\d+-[A-Z]`.
    (bar_dir / "1301.TSE-1-MINUTE-LAST-EXTERNAL").mkdir()
    (bar_dir / "7203.TSE-1-MINUTE-LAST-EXTERNAL").mkdir()
    (bar_dir / "backup").mkdir()  # must be skipped

    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    try:
        stub = _make_stub(port)
        resp = stub.ListInstruments(
            engine_pb2.ListInstrumentsRequest(token=TOKEN)
        )
        assert resp.success, resp.error_message
        # Legacy field still populated for backwards compat.
        assert sorted(resp.instrument_ids) == ["1301.TSE", "7203.TSE"]
        # New structured field — same ids, name falls back to id, market empty.
        got = sorted(resp.instruments, key=lambda i: i.id)
        assert [i.id for i in got] == ["1301.TSE", "7203.TSE"]
        assert [i.name for i in got] == ["1301.TSE", "7203.TSE"]
        assert [i.market for i in got] == ["", ""]
    finally:
        server.stop(0)


def test_list_instruments_wrong_token(grpc_server):
    port, _ = grpc_server
    stub = _make_stub(port)
    with pytest.raises(grpc.RpcError) as exc_info:
        stub.ListInstruments(
            engine_pb2.ListInstrumentsRequest(token="wrong-token")
        )
    assert exc_info.value.code() == grpc.StatusCode.UNAUTHENTICATED


# ---------------------------------------------------------------------------
# With catalog via jquants_catalog_path at startup
# ---------------------------------------------------------------------------

def _write_sample_catalog(catalog_path: Path) -> str:
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    catalog = ParquetDataCatalog(str(catalog_path.resolve()))
    bar_type = BarType.from_str("1301.TSE-1-MINUTE-LAST-EXTERNAL")

    def _bar(close: str, ts: int) -> Bar:
        p = Price.from_str(close)
        return Bar(
            bar_type=bar_type,
            open=p, high=p, low=p, close=p,
            volume=Quantity.from_int(100),
            ts_event=ts,
            ts_init=ts,
        )

    catalog.write_data([
        _bar("2000.00", 1_000_000_000),
        _bar("2010.00", 2_000_000_000),
    ])
    return str(bar_type)


@pytest.mark.slow
def test_list_instruments_with_jquants_catalog_path(tmp_path):
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    _write_sample_catalog(catalog_path)

    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    try:
        stub = _make_stub(port)
        resp = stub.ListInstruments(
            engine_pb2.ListInstrumentsRequest(token=TOKEN)
        )
        assert resp.success, resp.error_message
        assert "1301.TSE" in resp.instrument_ids
    finally:
        server.stop(0)


@pytest.mark.slow
def test_list_instruments_after_load_replay_data(tmp_path):
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    bar_type_str = _write_sample_catalog(catalog_path)

    engine = DataEngine()
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    try:
        stub = _make_stub(port)

        # Before LoadReplayData — no catalog yet
        resp = stub.ListInstruments(engine_pb2.ListInstrumentsRequest(token=TOKEN))
        assert not resp.success

        # LoadReplayData sets last_replay_catalog_path
        load_resp = stub.LoadReplayData(
            engine_pb2.LoadReplayDataRequest(
                request_id="r1",
                instrument_ids=[bar_type_str],
                granularity=engine_pb2.MINUTE,
                catalog_path=str(catalog_path),
                token=TOKEN,
            )
        )
        assert load_resp.success, load_resp.error_message

        # After LoadReplayData — catalog is set
        resp = stub.ListInstruments(engine_pb2.ListInstrumentsRequest(token=TOKEN))
        assert resp.success, resp.error_message
        assert "1301.TSE" in resp.instrument_ids
    finally:
        server.stop(0)
