from engine.proto import engine_pb2


def test_backend_event_has_backend_error_oneof():
    err = engine_pb2.BackendError(source="account_sync", detail="boom", ts_ms=123)
    assert err.source == "account_sync"
    assert err.detail == "boom"
    assert err.ts_ms == 123

    ev = engine_pb2.BackendEvent(
        backend_error=engine_pb2.BackendError(source="account_sync", detail="boom", ts_ms=123)
    )
    assert ev.WhichOneof("payload") == "backend_error"
