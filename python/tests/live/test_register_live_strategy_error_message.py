"""Slice 1 RED — RegisterLiveStrategyRes / StartLiveStrategyRes に error_message を詰める。

STRATEGY_LOAD_FAILED は "from exc" で raise されるが、現状の gRPC handler は
error_code しか返さず実際の例外メッセージ（__cause__）を捨てている。
このテストは error_message フィールドに原因が乗ることを確認する。
"""

from __future__ import annotations

import pytest

from engine.live.strategy_registry import StrategyRegistry, StrategyRegistryError


# ── helpers ──────────────────────────────────────────────────────────────


def _loader_raises(path: str, scenario: dict | None = None):
    """strategy_loader.load の代替: SyntaxError を模倣する。"""
    raise SyntaxError("unexpected indent at line 42")


# ── tests ─────────────────────────────────────────────────────────────────


def test_strategy_registry_error_preserves_cause():
    """StrategyRegistryError("STRATEGY_LOAD_FAILED") は __cause__ に元例外を持つ。

    これは既に実装済みの振る舞い（`raise ... from exc`）の確認。
    Slice 1 の実装で error_message を組み立てる際の根拠として残す。
    """
    registry = StrategyRegistry(loader=_loader_raises)

    # resolve する前に register が必要なので、実在ファイルを渡す必要がある。
    # strategy_file が見つからなければ STRATEGY_FILE_NOT_FOUND になるため、
    # __file__ 自身（このテストファイル）を渡す。
    with pytest.raises(StrategyRegistryError) as exc_info:
        registry.register(__file__, expected_sha256="")

    err = exc_info.value
    assert err.error_code == "STRATEGY_LOAD_FAILED"
    assert err.__cause__ is not None, "実例外が __cause__ に保持されていること"
    assert "unexpected indent" in str(err.__cause__), (
        f"__cause__ に元メッセージが含まれること: {err.__cause__}"
    )


def test_register_live_strategy_res_has_error_message_field():
    """RegisterLiveStrategyRes proto に error_message フィールドが存在すること。

    現状はフィールドが無いため AttributeError で RED になる。
    proto に `string error_message = 7;` を追加し、server_grpc が詰めれば GREEN になる。
    """
    from engine.proto import engine_pb2

    res = engine_pb2.RegisterLiveStrategyRes(
        success=False,
        request_id="req-001",
        error_code="STRATEGY_LOAD_FAILED",
        error_message="unexpected indent at line 42",  # ← このフィールドが存在するか
    )
    assert res.error_message == "unexpected indent at line 42"


def test_start_live_strategy_res_has_error_message_field():
    """StartLiveStrategyRes proto に error_message フィールドが存在すること。

    現状はフィールドが無いため AttributeError で RED になる。
    proto に `string error_message = 6;` を追加すれば GREEN になる。
    """
    from engine.proto import engine_pb2

    res = engine_pb2.StartLiveStrategyRes(
        success=False,
        request_id="req-002",
        error_code="STRATEGY_LOAD_FAILED",
        error_message="SyntaxError in strategy file",  # ← このフィールドが存在するか
    )
    assert res.error_message == "SyntaxError in strategy file"


def test_register_live_strategy_handler_returns_cause_error_message():
    """RegisterLiveStrategy handler が STRATEGY_LOAD_FAILED の実原因を response に詰める。"""
    import sys
    from pathlib import Path

    proto_dir = Path(__file__).resolve().parents[2] / "engine" / "proto"
    if str(proto_dir) not in sys.path:
        sys.path.insert(0, str(proto_dir))

    from engine.core import DataEngine
    from engine.live.engine_controller import NoopLiveEngineController
    from engine.proto import engine_pb2
    from engine.server_grpc import GrpcDataEngineServer

    servicer = GrpcDataEngineServer(
        "test-token",
        DataEngine(),
        engine_controller=NoopLiveEngineController(),
    )
    servicer._strategy_registry = StrategyRegistry(loader=_loader_raises)

    res = servicer.RegisterLiveStrategy(
        engine_pb2.RegisterLiveStrategyReq(
            token="test-token",
            request_id="req-handler",
            strategy_file=__file__,
            expected_sha256="",
        ),
        context=None,
    )

    assert not res.success
    assert res.request_id == "req-handler"
    assert res.error_code == "STRATEGY_LOAD_FAILED"
    assert "unexpected indent at line 42" in res.error_message


def test_start_live_strategy_handler_returns_cause_error_message():
    """StartLiveStrategy handler が LiveStrategyHostError の実原因を response に詰める。"""
    import sys
    from pathlib import Path
    from types import SimpleNamespace

    proto_dir = Path(__file__).resolve().parents[2] / "engine" / "proto"
    if str(proto_dir) not in sys.path:
        sys.path.insert(0, str(proto_dir))

    from engine.core import DataEngine
    from engine.live.engine_controller import NoopLiveEngineController
    from engine.live.strategy_host import LiveStrategyHostError
    from engine.proto import engine_pb2
    from engine.server_grpc import GrpcDataEngineServer

    class FakeModeManager:
        current_mode = "LiveAuto"

    class FakeRegistry:
        def resolve(self, strategy_id: str):
            return SimpleNamespace(
                strategy_id=strategy_id,
                resolved_path=__file__,
            )

    class FakeHost:
        def start_run(self, params):
            raise LiveStrategyHostError("STRATEGY_LOAD_FAILED") from SyntaxError(
                "invalid syntax at line 9"
            )

    servicer = GrpcDataEngineServer(
        "test-token",
        DataEngine(),
        mode_manager=FakeModeManager(),
        engine_controller=NoopLiveEngineController(),
    )
    servicer._strategy_registry = FakeRegistry()
    servicer._strategy_host = FakeHost()

    res = servicer.StartLiveStrategy(
        engine_pb2.StartLiveStrategyReq(
            token="test-token",
            request_id="start-handler",
            strategy_id="strat-test",
            instrument_id="7203.TSE",
            venue="MOCK",
        ),
        context=None,
    )

    assert not res.success
    assert res.request_id == "start-handler"
    assert res.error_code == "STRATEGY_LOAD_FAILED"
    assert "invalid syntax at line 9" in res.error_message
