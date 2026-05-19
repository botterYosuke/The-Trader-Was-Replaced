"""C-1 (plans/backend-startup-sync.md): Health.Check の service フィルタ契約。

- service="" / "DataEngine" → SERVING
- それ以外 → SERVICE_UNKNOWN
"""

from __future__ import annotations

import pytest

from engine.proto import engine_pb2, engine_pb2_grpc


@pytest.fixture
def health_stub():
    from concurrent import futures
    import grpc
    from engine.core import DataEngine
    from engine.server_grpc import GrpcDataEngineServer

    engine = DataEngine()
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer("test-token", engine)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    channel = grpc.insecure_channel(f"127.0.0.1:{port}")
    try:
        yield engine_pb2_grpc.HealthStub(channel)
    finally:
        channel.close()
        server.stop(0)


def test_health_check_default_service_returns_serving(health_stub) -> None:
    resp = health_stub.Check(engine_pb2.HealthCheckRequest(service=""), timeout=2.0)
    assert resp.status == engine_pb2.HealthCheckResponse.SERVING


def test_health_check_dataengine_service_returns_serving(health_stub) -> None:
    resp = health_stub.Check(
        engine_pb2.HealthCheckRequest(service="DataEngine"), timeout=2.0
    )
    assert resp.status == engine_pb2.HealthCheckResponse.SERVING


def test_health_check_unknown_service_returns_service_unknown(health_stub) -> None:
    resp = health_stub.Check(
        engine_pb2.HealthCheckRequest(service="OtherService"), timeout=2.0
    )
    assert resp.status == engine_pb2.HealthCheckResponse.SERVICE_UNKNOWN
