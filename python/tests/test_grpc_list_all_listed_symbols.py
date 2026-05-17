"""
gRPC ListAllListedSymbols — Phase 7.5b RED skeleton.

Step 1: 1 / 2 / 8 を具体 assertion で RED 化。3〜7 は handler 実装 (サブ D) まで skip。
"""

from concurrent import futures
from pathlib import Path

import grpc
import pandas as pd
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


def _write_sample_catalog(catalog_path: Path, ts_ns: int | None = None) -> str:
    """Write a minimal 2-bar catalog. ts_ns: base ns for the first bar; second is +1s."""
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

    if ts_ns is None:
        ts0, ts1 = 1_000_000_000, 2_000_000_000
    else:
        ts0, ts1 = ts_ns, ts_ns + 1_000_000_000

    catalog.write_data([
        _bar("2000.00", ts0),
        _bar("2010.00", ts1),
    ])
    return str(bar_type)


# ---------------------------------------------------------------------------
# Case 1: token 不正 → UNAUTHENTICATED
# ---------------------------------------------------------------------------

def test_list_all_listed_symbols_wrong_token(grpc_server):
    port, _ = grpc_server
    stub = _make_stub(port)
    with pytest.raises(grpc.RpcError) as exc_info:
        stub.ListAllListedSymbols(
            engine_pb2.ListAllListedSymbolsRequest(token="wrong-token", end_date="")
        )
    assert exc_info.value.code() == grpc.StatusCode.UNAUTHENTICATED


# ---------------------------------------------------------------------------
# Case 2: end_date="" + catalog 設定済 → success, "1301.TSE" in ids,
#         resolved_end_date 非空, artifact ファイル生成
# ---------------------------------------------------------------------------

@pytest.mark.slow
def test_list_all_listed_symbols_empty_end_date_with_catalog(tmp_path, monkeypatch):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    ts_ns = pd.Timestamp("2024-01-04", tz="UTC").value
    _write_sample_catalog(catalog_path, ts_ns=ts_ns)

    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    try:
        stub = _make_stub(port)
        resp = stub.ListAllListedSymbols(
            engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="")
        )
        assert resp.success, resp.error_message
        assert "1301.TSE" in resp.instrument_ids
        assert resp.resolved_end_date != ""

        artifact_dir = tmp_path / "artifacts" / "instrument-lists"
        assert artifact_dir.exists(), "artifact dir not created"
        artifacts = list(artifact_dir.glob("*"))
        assert artifacts, f"no artifact file under {artifact_dir}"
    finally:
        server.stop(0)


# ---------------------------------------------------------------------------
# Case 3: artifact miss → catalog 走査 → artifact write
# ---------------------------------------------------------------------------

@pytest.mark.slow
def test_list_all_listed_symbols_artifact_miss_then_write(tmp_path, monkeypatch):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    ts_ns = pd.Timestamp("2024-01-04", tz="UTC").value
    _write_sample_catalog(catalog_path, ts_ns=ts_ns)

    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    try:
        artifact_path = tmp_path / "artifacts" / "instrument-lists" / "listed-symbols-2024-01-04.json"
        assert not artifact_path.exists()
        stub = _make_stub(port)
        resp = stub.ListAllListedSymbols(
            engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="2024-01-04")
        )
        assert resp.success, resp.error_message
        assert "1301.TSE" in resp.instrument_ids
        assert resp.resolved_end_date == "2024-01-04"
        assert artifact_path.exists()
        import json as _json
        payload = _json.loads(artifact_path.read_text(encoding="utf-8"))
        assert payload["schema_version"] == 1
        assert payload["end_date"] == "2024-01-04"
        assert "1301.TSE" in payload["instrument_ids"]
    finally:
        server.stop(0)


# ---------------------------------------------------------------------------
# Case 4: artifact hit → catalog 無くても success
# ---------------------------------------------------------------------------

def test_list_all_listed_symbols_artifact_hit_no_catalog(grpc_server, tmp_path, monkeypatch):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    artifact_dir = tmp_path / "artifacts" / "instrument-lists"
    artifact_dir.mkdir(parents=True)
    artifact_path = artifact_dir / "listed-symbols-2024-01-04.json"
    import json as _json
    artifact_path.write_text(
        _json.dumps({
            "schema_version": 1,
            "end_date": "2024-01-04",
            "source": "nautilus_catalog",
            "catalog_path": "",
            "generated_at": "2024-01-04T00:00:00Z",
            "instrument_ids": ["9999.TSE", "8888.TSE"],
        }),
        encoding="utf-8",
    )
    port, _ = grpc_server
    stub = _make_stub(port)
    resp = stub.ListAllListedSymbols(
        engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="2024-01-04")
    )
    assert resp.success, resp.error_message
    assert resp.resolved_end_date == "2024-01-04"
    assert list(resp.instrument_ids) == ["9999.TSE", "8888.TSE"]


# ---------------------------------------------------------------------------
# Case 5: invalid artifact → 再生成
# ---------------------------------------------------------------------------

@pytest.mark.slow
@pytest.mark.parametrize("bad_payload", [
    '{"schema_version": 2, "end_date": "2024-01-04", "instrument_ids": ["X.TSE"]}',
    '{"schema_version": 1, "end_date": "2023-12-31", "instrument_ids": ["X.TSE"]}',
    '{not valid json',
])
def test_list_all_listed_symbols_invalid_artifact_regenerate(tmp_path, monkeypatch, bad_payload):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    ts_ns = pd.Timestamp("2024-01-04", tz="UTC").value
    _write_sample_catalog(catalog_path, ts_ns=ts_ns)
    artifact_dir = tmp_path / "artifacts" / "instrument-lists"
    artifact_dir.mkdir(parents=True)
    artifact_path = artifact_dir / "listed-symbols-2024-01-04.json"
    artifact_path.write_text(bad_payload, encoding="utf-8")
    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    try:
        stub = _make_stub(port)
        resp = stub.ListAllListedSymbols(
            engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="2024-01-04")
        )
        assert resp.success, resp.error_message
        assert "1301.TSE" in resp.instrument_ids
        assert resp.resolved_end_date == "2024-01-04"
        import json as _json
        payload = _json.loads(artifact_path.read_text(encoding="utf-8"))
        assert payload["schema_version"] == 1
        assert payload["end_date"] == "2024-01-04"
        assert "1301.TSE" in payload["instrument_ids"]
    finally:
        server.stop(0)


# ---------------------------------------------------------------------------
# Case 6: catalog 最古より前の日付 → success, ids=[]
# ---------------------------------------------------------------------------

@pytest.mark.slow
def test_list_all_listed_symbols_date_before_oldest(tmp_path, monkeypatch):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    ts_ns = pd.Timestamp("2024-01-04", tz="UTC").value
    _write_sample_catalog(catalog_path, ts_ns=ts_ns)
    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    try:
        stub = _make_stub(port)
        resp = stub.ListAllListedSymbols(
            engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="1999-01-01")
        )
        assert resp.success, resp.error_message
        assert list(resp.instrument_ids) == []
        assert resp.resolved_end_date == "1999-01-01"
        artifact_path = tmp_path / "artifacts" / "instrument-lists" / "listed-symbols-1999-01-01.json"
        assert artifact_path.exists()
        import json as _json
        payload = _json.loads(artifact_path.read_text(encoding="utf-8"))
        assert payload["instrument_ids"] == []
        assert payload["end_date"] == "1999-01-01"
    finally:
        server.stop(0)


# ---------------------------------------------------------------------------
# Case 7: 未来日付 → resolved = catalog 最新日
# ---------------------------------------------------------------------------

@pytest.mark.slow
def test_list_all_listed_symbols_future_date_clamps_to_latest(tmp_path, monkeypatch):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    ts_ns = pd.Timestamp("2024-01-04", tz="UTC").value
    _write_sample_catalog(catalog_path, ts_ns=ts_ns)
    engine = DataEngine(jquants_catalog_path=str(catalog_path))
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(TOKEN, engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    try:
        stub = _make_stub(port)
        resp = stub.ListAllListedSymbols(
            engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="2099-12-31")
        )
        assert resp.success, resp.error_message
        assert resp.resolved_end_date == "2024-01-04"
        assert "1301.TSE" in resp.instrument_ids
        artifact_path = tmp_path / "artifacts" / "instrument-lists" / "listed-symbols-2024-01-04.json"
        assert artifact_path.exists()
    finally:
        server.stop(0)


# ---------------------------------------------------------------------------
# Case 8: artifact miss + catalog 未設定 → success=false, error_message に
#         "No catalog_path available"
# ---------------------------------------------------------------------------

def test_list_all_listed_symbols_no_catalog_no_artifact(grpc_server, tmp_path, monkeypatch):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path / "artifacts"))
    port, _ = grpc_server
    stub = _make_stub(port)
    resp = stub.ListAllListedSymbols(
        engine_pb2.ListAllListedSymbolsRequest(token=TOKEN, end_date="")
    )
    assert not resp.success
    assert "No catalog_path available" in resp.error_message
