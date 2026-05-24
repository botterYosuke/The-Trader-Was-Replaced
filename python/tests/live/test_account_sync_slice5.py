"""Slice 5 (#29): 30s AccountSync の存続と差分 re-emit 保証。

テスト対象:
1. _teardown_live_components_async が account_sync.stop() を呼ぶ（Replay 遷移時に止まる）
2. SetExecutionMode Live→Live では teardown されず account_sync が生き続ける
3. dedup: 変化のない snapshot は re-emit されない（account_sync 本体の不変条件）
"""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from engine.core import DataEngine
from engine.live.account_sync import AccountSync
from engine.live.adapter import VenueCredentials
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.order_types import AccountSnapshot
from engine.mode_manager import ModeManager
from engine.live.state_machine import VenueStateMachine
from engine.server_grpc import GrpcDataEngineServer


def _make_servicer(token: str = "tok") -> GrpcDataEngineServer:
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)
    return GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)


# ---------------------------------------------------------------------------
# 1. teardown が account_sync.stop() を呼ぶ（Replay 遷移のパス）
# ---------------------------------------------------------------------------

def test_teardown_async_stops_account_sync() -> None:
    """_teardown_live_components_async は account_sync.stop() を 1 回呼ぶ。"""
    servicer = _make_servicer()
    mock_sync = MagicMock()
    mock_sync.stop = AsyncMock()
    servicer._account_sync = mock_sync
    # bridge/runner を None にして他の stop() が走らないようにする
    servicer._live_runner = None
    servicer._live_bridge = None
    servicer._live_price_cache = None
    servicer._live_depth_cache = None
    servicer._health_watchdog = None
    servicer._instruments_scheduler = None

    asyncio.run(servicer._teardown_live_components_async())

    mock_sync.stop.assert_called_once()


# ---------------------------------------------------------------------------
# 2. Live→Live 遷移では teardown せず account_sync が残る
# ---------------------------------------------------------------------------

def test_set_execution_mode_live_to_live_preserves_account_sync() -> None:
    """LiveManual→LiveAuto 遷移は _teardown_live_components を呼ばず account_sync が生き続ける。

    SetExecutionMode の teardown ゲート条件（applied == "Replay"）を回帰ガードする。
    """
    from engine.proto import engine_pb2

    servicer = _make_servicer(token="tok")
    mock_sync = MagicMock()
    servicer._account_sync = mock_sync
    # live_runner を非 None にして D21 ガードを通過させる
    servicer._live_runner = MagicMock()
    # live_adapter_factory を非 None にして LIVE_ADAPTER_NOT_CONFIGURED を回避する
    servicer._live_adapter_factory = MagicMock()

    class _Req:
        mode = "LiveAuto"
        token = "tok"

    class _Ctx:
        def abort(self, code, msg):
            raise RuntimeError(f"abort: {code} {msg}")
        def peer(self):
            return "ipv4:127.0.0.1:0"

    with patch.object(servicer.mode_manager, "set_execution_mode", return_value="LiveAuto"), \
         patch.object(servicer, "_teardown_live_components") as mock_teardown:
        servicer.SetExecutionMode(_Req(), _Ctx())

    mock_teardown.assert_not_called()
    assert servicer._account_sync is mock_sync


# ---------------------------------------------------------------------------
# 3. dedup: 同一 snapshot は re-emit されない / 変化したら emit される
#    (account_sync 本体の不変条件 — test_account_sync.py の補完)
# ---------------------------------------------------------------------------

def test_account_sync_does_not_emit_on_unchanged_snapshot() -> None:
    """30s ポーリングで snapshot が変化しない場合は re-emit されない（差分 push 保証）。"""

    async def scenario() -> int:
        adapter = MockVenueAdapter()
        await adapter.login(VenueCredentials(credentials_source="env", environment_hint="demo"))
        adapter.set_account_snapshot(cash=100.0, buying_power=200.0, positions=[])
        seen: list[AccountSnapshot] = []
        sync = AccountSync(adapter, on_account_event=seen.append, interval_s=0.01)
        await sync.start()
        # 初回 emit を待つ
        for _ in range(30):
            if seen:
                break
            await asyncio.sleep(0.005)
        first_count = len(seen)
        # 不変のまま複数 interval を待つ
        await asyncio.sleep(0.08)
        count_after_idle = len(seen)
        await sync.stop()
        return count_after_idle - first_count

    extra_emits = asyncio.run(scenario())
    assert extra_emits == 0, f"snapshot 不変なのに {extra_emits} 件余分に emit された"
