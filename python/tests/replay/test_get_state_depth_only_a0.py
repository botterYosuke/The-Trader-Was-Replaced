"""Finding 3: GetState surfaces depth-only instruments (no kline yet).

Live mode の GetState は、DepthUpdate は届いたが kline (per_instrument 登録) が
まだ無い銘柄も per_instrument に union で surface しなければならない。
"""
from __future__ import annotations

import json
from concurrent import futures

import grpc
import pytest

from engine.core import DataEngine
from engine.live.live_adapter_factory import build_live_adapter_factory
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.models import DepthLevel, DepthSnapshot
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


class _StubDepthCache:
    """DepthCache.snapshot() 互換: dict[str, DepthSnapshot] を返す。"""

    def __init__(self, snap: dict[str, DepthSnapshot]) -> None:
        self._snap = snap

    def snapshot(self) -> dict[str, DepthSnapshot]:
        return dict(self._snap)


@pytest.fixture
def live_server():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    live_adapter_factory = build_live_adapter_factory("MOCK")

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(
        token,
        engine,
        mode_manager=mm,
        venue_sm=venue_sm,
        live_adapter_factory=live_adapter_factory,
        live_venue_id="MOCK",
    )
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine, venue_sm, mm, servicer)

    if servicer._live_runner is not None or servicer._live_bridge is not None:
        servicer._teardown_live_components()
    server.stop(0)


def _stub(port):
    channel = grpc.insecure_channel(f"localhost:{port}")
    return engine_pb2_grpc.DataEngineStub(channel)


def _do_venue_login(stub, token):
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="MOCK",
            credentials_source="env",
            token=token,
        )
    )
    assert resp.success is True, f"VenueLogin failed: {resp.error_code}"


def test_get_state_surfaces_depth_only_instrument(live_server):
    port, token, engine, venue_sm, mm, servicer = live_server
    stub = _stub(port)

    # D21: VenueLogin must precede SetExecutionMode for Live modes
    _do_venue_login(stub, token)
    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True

    # depth-only 銘柄 (kline 未着 = per_instrument に居ない) を depth cache に注入
    depth_only = DepthSnapshot(
        bids=[DepthLevel(price=100.0, size=1.0)],
        asks=[DepthLevel(price=102.0, size=1.0)],
        timestamp_ms=1_000,
    )
    servicer._live_depth_cache = _StubDepthCache({"7203.TSE": depth_only})

    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    pi = payload["per_instrument"]

    assert "7203.TSE" in pi
    assert pi["7203.TSE"]["depth"]["bids"][0]["price"] == 100.0
    assert pi["7203.TSE"]["price"] is None
    assert pi["7203.TSE"]["ohlc_points"] == []
