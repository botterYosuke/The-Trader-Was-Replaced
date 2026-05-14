"""gRPC GetPortfolio — unit tests."""
from concurrent import futures

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
    port = server.add_insecure_port("[::]:0")
    server.start()
    yield port, engine
    server.stop(0)


def _stub(port: int):
    return engine_pb2_grpc.DataEngineStub(grpc.insecure_channel(f"localhost:{port}"))


# ── auth ──────────────────────────────────────────────────────────────────────

def test_get_portfolio_wrong_token(grpc_server):
    port, _ = grpc_server
    with pytest.raises(grpc.RpcError) as exc:
        _stub(port).GetPortfolio(engine_pb2.GetPortfolioRequest(token="bad"))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


# ── empty state (before any run) ──────────────────────────────────────────────

def test_get_portfolio_empty_before_run(grpc_server):
    port, _ = grpc_server
    resp = _stub(port).GetPortfolio(engine_pb2.GetPortfolioRequest(token=TOKEN))
    assert resp.success
    assert resp.buying_power == pytest.approx(0.0)
    assert resp.cash == pytest.approx(0.0)
    assert resp.equity == pytest.approx(0.0)
    assert list(resp.positions) == []
    assert list(resp.orders) == []


# ── injected state ────────────────────────────────────────────────────────────

def test_get_portfolio_reflects_injected_buying_power(grpc_server):
    port, engine = grpc_server
    engine.last_portfolio = {
        "buying_power": 9_000_000.0,
        "cash": 9_000_000.0,
        "equity": 9_500_000.0,
        "positions": [],
        "orders": [],
    }
    resp = _stub(port).GetPortfolio(engine_pb2.GetPortfolioRequest(token=TOKEN))
    assert resp.success
    assert resp.buying_power == pytest.approx(9_000_000.0)
    assert resp.equity == pytest.approx(9_500_000.0)


def test_get_portfolio_reflects_injected_positions(grpc_server):
    port, engine = grpc_server
    engine.last_portfolio = {
        "buying_power": 8_000_000.0,
        "cash": 8_000_000.0,
        "equity": 8_100_000.0,
        "positions": [
            {"symbol": "1301.TSE", "qty": 100, "avg_price": 4000.0, "unrealized_pnl": 10000.0}
        ],
        "orders": [],
    }
    resp = _stub(port).GetPortfolio(engine_pb2.GetPortfolioRequest(token=TOKEN))
    assert resp.success
    assert len(resp.positions) == 1
    pos = resp.positions[0]
    assert pos.symbol == "1301.TSE"
    assert pos.qty == 100
    assert pos.avg_price == pytest.approx(4000.0)
    assert pos.unrealized_pnl == pytest.approx(10000.0)


def test_get_portfolio_reflects_injected_orders(grpc_server):
    port, engine = grpc_server
    engine.last_portfolio = {
        "buying_power": 0.0,
        "cash": 0.0,
        "equity": 0.0,
        "positions": [],
        "orders": [
            {"symbol": "1301.TSE", "side": "BUY", "qty": 100.0, "price": 4000.0, "status": "FILLED", "ts_ms": 1_700_000_000_000},
            {"symbol": "1301.TSE", "side": "SELL", "qty": 100.0, "price": 4100.0, "status": "FILLED", "ts_ms": 1_700_000_001_000},
        ],
    }
    resp = _stub(port).GetPortfolio(engine_pb2.GetPortfolioRequest(token=TOKEN))
    assert resp.success
    assert len(resp.orders) == 2
    assert resp.orders[0].side == "BUY"
    assert resp.orders[0].price == pytest.approx(4000.0)
    assert resp.orders[1].side == "SELL"
    assert resp.orders[1].ts_ms == 1_700_000_001_000
