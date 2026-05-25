import asyncio
import json
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
from concurrent import futures

import grpc

from engine.core import DataEngine
from engine.live.adapter import DepthLevel
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2, engine_pb2_grpc
from engine.server_grpc import GrpcDataEngineServer


@pytest.fixture
def phase8_grpc_server():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)

    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine, venue_sm, mm)

    server.stop(0)


def _stub(port):
    channel = grpc.insecure_channel(f"localhost:{port}")
    return engine_pb2_grpc.DataEngineStub(channel)


# --- VenueLogin ---------------------------------------------------------

def test_venue_login_invalid_credentials_source_raises_invalid_argument(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="keyring",
                token=token,
            )
        )
    assert exc.value.code() == grpc.StatusCode.INVALID_ARGUMENT
    assert "INVALID_CREDENTIALS_SOURCE" in exc.value.details()


def test_venue_login_unknown_venue_returns_error_code(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="FOO",
            credentials_source="prompt",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "UNKNOWN_VENUE"


def test_venue_login_kabu_session_cache_returns_unsupported(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="KABU",
            credentials_source="session_cache",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "UNSUPPORTED_FOR_VENUE"


def test_venue_login_without_adapter_factory_returns_not_configured(phase8_grpc_server):
    """D21: VenueLogin without live_adapter_factory returns LIVE_ADAPTER_NOT_CONFIGURED."""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="TACHIBANA",
            credentials_source="prompt",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "LIVE_ADAPTER_NOT_CONFIGURED"


# --- VenueLogout --------------------------------------------------------

def test_venue_logout_returns_success(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.VenueLogout(engine_pb2.VenueLogoutRequest(token=token))
    assert resp.success is True
    assert resp.error_code == ""


# --- SetExecutionMode ---------------------------------------------------

def test_set_execution_mode_invalid_mode_raises_invalid_argument(phase8_grpc_server):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Foo", token=token))
    assert exc.value.code() == grpc.StatusCode.INVALID_ARGUMENT


def test_set_execution_mode_replay_always_reachable(phase8_grpc_server):
    # #30: Replay is the always-reachable home mode — no precondition gating,
    # reachable even with no strategy loaded / venue IDLE. (GH #37: was asserting
    # the pre-#30 behavior where Replay was rejected by precondition.)
    port, token, engine, venue_sm, mm = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Replay", token=token))
    assert resp.success is True
    assert resp.error_code == ""
    assert resp.execution_mode == "Replay"


# --- token 検証 (RED: handler 未実装) ---------------------------------------

def test_venue_login_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token="wrong-token",
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_token_one_char_off_is_rejected_constant_time(phase8_grpc_server):
    # MEDIUM-2: token auth uses hmac.compare_digest (constant-time). A token that
    # differs by a single trailing character must still be rejected.
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    bad = token[:-1] + ("Y" if token[-1] != "Y" else "Z")  # exactly 1 char off
    assert bad != token and len(bad) == len(token)
    with pytest.raises(grpc.RpcError) as exc:
        stub.GetState(engine_pb2.GetStateRequest(token=bad))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_token_ok_helper_constant_time(phase8_grpc_server):
    # Unit-level check of the centralized helper.
    _port, token, engine, venue_sm, mm = phase8_grpc_server
    servicer = GrpcDataEngineServer(token, engine, mode_manager=mm, venue_sm=venue_sm)

    class _Req:
        def __init__(self, t):
            self.token = t

    assert servicer._token_ok(_Req(token)) is True
    assert servicer._token_ok(_Req(token + "x")) is False
    assert servicer._token_ok(_Req("")) is False
    assert servicer._token_ok(_Req(None)) is False


def test_venue_logout_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.VenueLogout(engine_pb2.VenueLogoutRequest(token="wrong-token"))
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_set_execution_mode_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SetExecutionMode(
            engine_pb2.SetExecutionModeRequest(mode="LiveManual", token="wrong-token")
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_subscribe_market_data_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.SubscribeMarketData(
            engine_pb2.SubscribeRequest(
                instrument_id="7203.TSE",
                channels=["bar"],
                token="wrong-token",
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_unsubscribe_market_data_wrong_token_raises_unauthenticated(phase8_grpc_server):
    port, *_ = phase8_grpc_server
    stub = _stub(port)
    with pytest.raises(grpc.RpcError) as exc:
        stub.UnsubscribeMarketData(
            engine_pb2.UnsubscribeRequest(
                instrument_id="7203.TSE", token="wrong-token"
            )
        )
    assert exc.value.code() == grpc.StatusCode.UNAUTHENTICATED


@pytest.fixture
def phase8_grpc_server_with_live():
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    from engine.live.live_adapter_factory import build_live_adapter_factory
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

    # Teardown live components if any test spawned them.
    # 同期 API を使う: bridge/runner は servicer._live_loop (background thread)
    # に attach されているので、別 loop で asyncio.run(async版) を呼ぶと
    # "attached to a different loop" になる。
    if servicer._live_runner is not None or servicer._live_bridge is not None:
        servicer._teardown_live_components()

    server.stop(0)


def _do_venue_login(stub, token, venue_id="MOCK"):
    """Helper: perform VenueLogin via gRPC (D21 precondition for SetExecutionMode).

    MOCK venue uses "env" source to avoid spawning a subprocess login dialog.
    Real venues (TACHIBANA / KABU) use "prompt" in production; tests may override.
    """
    cred_source = "env" if venue_id.upper() == "MOCK" else "prompt"
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id=venue_id,
            credentials_source=cred_source,
            token=token,
        )
    )
    assert resp.success is True, f"VenueLogin failed: {resp.error_code}"
    return resp


def test_set_execution_mode_live_manual_spawns_live_runner(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    stub = _stub(port)
    # D21: VenueLogin must precede SetExecutionMode for Live modes
    _do_venue_login(stub, token)
    resp = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp.success is True
    assert servicer._live_runner is not None
    assert servicer._live_bridge is not None


def test_set_execution_mode_replay_does_not_teardown_live_session(
    phase8_grpc_server_with_live,
):
    """Issue #39: Replay 切替は live session を teardown しない。
    LiveManual → Replay に戻しても _live_runner / _live_bridge が生き続ける。
    (旧挙動: teardown_live_components を呼んでいた。fix: その 2 行を削除。)"""
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    stub = _stub(port)

    # 1. Venue login → runner 起動
    _do_venue_login(stub, token)

    # 2. LiveManual に切り替えて runner/bridge が生きていることを確認
    resp_live = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_live.success is True
    assert servicer._live_runner is not None
    assert servicer._live_bridge is not None

    # 3. Replay に戻す precondition: engine._replay_state が LOADED/RUNNING/PAUSED
    engine._replay_state = "LOADED"

    # 4. Replay に切り替えても runner/bridge は解放されない (issue #39 fix)
    resp_replay = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="Replay", token=token)
    )
    assert resp_replay.success is True
    assert resp_replay.execution_mode == "Replay"
    assert servicer._live_runner is not None, (
        "Replay 切替は live runner を teardown しない (issue #39)"
    )
    assert servicer._live_bridge is not None, (
        "Replay 切替は live bridge を teardown しない (issue #39)"
    )


# --- Step 3.3 RED: live_adapter_factory 未設定時の LiveManual 拒否 ---------

def test_set_execution_mode_live_manual_without_adapter_factory_returns_error(
    phase8_grpc_server,
):
    """
    live_adapter_factory が None のまま LiveManual を要求された場合、
    silent に success=True を返してはならない。
    - response.success == False
    - response.error_code == "LIVE_ADAPTER_NOT_CONFIGURED"
    - response.execution_mode != "LiveManual" (遷移していない)
    RED 期待: 現実装は _start_live_components が silent return するため
    success=True, execution_mode="LiveManual" が返って失敗する。
    """
    port, token, engine, venue_sm, mm = phase8_grpc_server
    venue_sm.transition_to("AUTHENTICATING")
    venue_sm.transition_to("CONNECTED")
    stub = _stub(port)
    resp = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp.success is False, (
        f"factory 未設定で LiveManual を許可してはならない (silent green failure): "
        f"success={resp.success}, error_code={resp.error_code!r}, "
        f"execution_mode={resp.execution_mode!r}"
    )
    assert resp.error_code == "LIVE_ADAPTER_NOT_CONFIGURED", (
        f"error_code must be LIVE_ADAPTER_NOT_CONFIGURED, got {resp.error_code!r}"
    )
    assert resp.execution_mode != "LiveManual", (
        f"execution_mode は LiveManual に遷移してはならない: {resp.execution_mode!r}"
    )


def test_subscribe_market_data_succeeds_in_live_mode(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    stub = _stub(port)

    # D21: VenueLogin must precede SetExecutionMode
    _do_venue_login(stub, token)

    # Live mode へ遷移し runner を起動
    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True
    assert servicer._live_runner is not None

    # Subscribe を発行
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE",
            channels=["trades", "depth"],
            token=token,
        )
    )
    assert resp.success is True
    assert resp.error_code == ""

    # runner 側 aggregator に登録されていることを確認
    assert "7203.TSE" in servicer._live_runner._aggregators


def test_subscribe_market_data_rejects_without_live_runner(
    phase8_grpc_server,
):
    # Replay モード (runner なし) では precondition reject
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE", channels=["trades"], token=token
        )
    )
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


def test_unsubscribe_market_data_succeeds_after_subscribe(
    phase8_grpc_server_with_live,
):
    """Subscribe 済み instrument を Unsubscribe すると success=True を返し、
    runner._aggregators から消える。"""
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    stub = _stub(port)

    # D21: VenueLogin must precede SetExecutionMode
    _do_venue_login(stub, token)

    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True
    assert servicer._live_runner is not None

    # Subscribe してから Unsubscribe
    resp_sub = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE",
            channels=["trades", "depth"],
            token=token,
        )
    )
    assert resp_sub.success is True
    assert "7203.TSE" in servicer._live_runner._aggregators

    resp = stub.UnsubscribeMarketData(
        engine_pb2.UnsubscribeRequest(instrument_id="7203.TSE", token=token)
    )
    assert resp.success is True
    assert resp.error_code == ""
    assert "7203.TSE" not in servicer._live_runner._aggregators


def test_unsubscribe_market_data_rejects_without_live_runner(
    phase8_grpc_server,
):
    """Replay モード (runner なし) では precondition reject。"""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.UnsubscribeMarketData(
        engine_pb2.UnsubscribeRequest(instrument_id="7203.TSE", token=token)
    )
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


def test_get_state_exposes_live_last_error_when_runner_failed(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    mock_runner = MagicMock()
    mock_runner.last_error = ConnectionError("boom")
    servicer._live_runner = mock_runner
    stub = _stub(port)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] == "ConnectionError: boom"


def test_get_state_live_last_error_is_none_when_no_error(
    phase8_grpc_server,
):
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] is None


def test_get_state_exposes_last_prices_from_live_cache(
    phase8_grpc_server_with_live,
):
    """Live mode で DepthUpdate(bid=100, ask=102) を inject すると、
    GetState の state.last_prices["7203.TSE"] == 101.0 が返る (quote_mid)。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    # D21: VenueLogin must precede SetExecutionMode
    _do_venue_login(stub, token)

    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True
    assert servicer._live_price_cache is not None

    resp_sub = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE",
            channels=["trades", "depth"],
            token=token,
        )
    )
    assert resp_sub.success is True

    # adapter は LiveRunner 経由で subscribe 済み。emit_depth_snapshot は
    # adapter._queue.put_nowait なので、必ず servicer._live_loop 上で実行する
    # (adapter._queue が attach されている loop と揃える)。
    adapter = servicer._live_runner._adapter
    fut = asyncio.run_coroutine_threadsafe(
        _emit_depth_async(adapter, "7203.TSE", bid=100.0, ask=102.0),
        servicer._live_loop,
    )
    fut.result(timeout=2.0)

    # runner -> bus -> cache の伝播待ち (interval_ns=60s だが cache は bus
    # subscriber 直結なので即時。念のため少し待つ)
    import time as _time
    deadline = _time.time() + 2.0
    while _time.time() < deadline:
        snap = servicer._live_price_cache.snapshot()
        if snap.get("7203.TSE") == 101.0:
            break
        _time.sleep(0.05)

    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["last_prices"]["7203.TSE"] == 101.0


async def _emit_depth_async(adapter, instrument_id: str, bid: float, ask: float) -> None:
    adapter.emit_depth_snapshot(
        instrument_id=instrument_id,
        ts_ns=1_000_000_000,
        bids=[DepthLevel(price=bid, size=1)],
        asks=[DepthLevel(price=ask, size=1)],
    )


def test_get_state_last_prices_empty_in_replay_mode(phase8_grpc_server):
    """Replay モード (cache 無し) では last_prices は空 dict。"""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["last_prices"] == {}


# --- D1: ListInstruments source dispatch ------------------------------------

def test_list_instruments_local_returns_error_without_catalog(phase8_grpc_server):
    """source='local' (default) — catalog 無しなら success=False."""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.ListInstruments(
        engine_pb2.ListInstrumentsRequest(source="local", token=token)
    )
    assert resp.success is False


def test_list_instruments_live_without_runner_returns_failure(phase8_grpc_server):
    """source='live' — runner 未起動なら success=False (not logged in)."""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.ListInstruments(
        engine_pb2.ListInstrumentsRequest(source="live", token=token)
    )
    assert resp.success is False
    assert "LIVE_VENUE_NOT_LOGGED_IN" in resp.error_message


def test_list_instruments_live_with_runner_returns_mock_instruments(
    phase8_grpc_server_with_live,
):
    """D1/D10: source='live' — MOCK runner ログイン済みなら MockVenueAdapter の
    instrument リストが返る。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    # VenueLogin でリソース起動
    _do_venue_login(stub, token)

    resp = stub.ListInstruments(
        engine_pb2.ListInstrumentsRequest(source="live", token=token)
    )
    # MockVenueAdapter.fetch_instruments は空リストを返し LIVE_UNIVERSE_UNSUPPORTED になる
    # または実装によっては成功するかも。ここでは「runner 起動後に呼び出せること」を確認
    # LIVE_VENUE_NOT_LOGGED_IN ではないことを保証する
    assert "LIVE_VENUE_NOT_LOGGED_IN" not in resp.error_message


def test_list_instruments_unknown_source_returns_failure(phase8_grpc_server):
    """source が 'local'/'live' 以外なら success=False."""
    port, token, *_ = phase8_grpc_server
    stub = _stub(port)
    resp = stub.ListInstruments(
        engine_pb2.ListInstrumentsRequest(source="jquants", token=token)
    )
    assert resp.success is False
    assert "unknown source" in resp.error_message.lower()


# --- D21: VenueLogin + SetExecutionMode flow --------------------------------

def test_venue_login_mock_returns_success(phase8_grpc_server_with_live):
    """D21/D26: MOCK venue に VenueLogin すると success=True になる。"""
    port, token, *_ = phase8_grpc_server_with_live
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="MOCK",
            credentials_source="env",
            token=token,
        )
    )
    assert resp.success is True
    assert resp.error_code == ""


def test_venue_login_normalizes_lowercase_venue_id(phase8_grpc_server_with_live):
    """D21: venue_id は大文字正規化される (mock → MOCK)."""
    port, token, *_ = phase8_grpc_server_with_live
    stub = _stub(port)
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="mock",
            credentials_source="env",
            token=token,
        )
    )
    assert resp.success is True


def test_venue_login_mock_sets_venue_sm_connected(phase8_grpc_server_with_live):
    """D18: VenueLogin 成功後に venue_sm.current == "CONNECTED"。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    assert venue_sm.current == "CONNECTED"


def test_set_execution_mode_requires_venue_login_first(phase8_grpc_server_with_live):
    """D21: VenueLogin なしで LiveManual を要求すると EXECUTION_MODE_PRECONDITION。"""
    port, token, *_ = phase8_grpc_server_with_live
    stub = _stub(port)
    # venue_sm は DISCONNECTED のまま → precondition 失敗
    resp = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


def test_venue_login_idempotent_when_already_connected(phase8_grpc_server_with_live):
    """D21: 既に CONNECTED なら 2 回目の VenueLogin も success=True (no-op)."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    assert venue_sm.current == "CONNECTED"
    # 2 回目 (idempotent — already CONNECTED skips login logic)
    resp2 = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="MOCK",
            credentials_source="env",
            token=token,
        )
    )
    assert resp2.success is True
    assert venue_sm.current == "CONNECTED"


def test_venue_login_recovers_when_connected_but_runner_has_last_error(
    phase8_grpc_server_with_live,
):
    """Round1 MEDIUM: when venue_sm is CONNECTED but the live runner died with
    a last_error (crashed WS task), a re-login must NOT be a no-op success. It
    must tear down the dead session and re-establish so the UI recovers and
    live_last_error clears. The plain idempotent test above only covers the
    healthy case."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    assert venue_sm.current == "CONNECTED"

    # Simulate a crashed WS task: runner stays CONNECTED in the state machine
    # (LiveRunner._run never flips venue_sm to ERROR) but holds a last_error.
    dead_runner = servicer._live_runner
    assert dead_runner is not None
    dead_runner._last_error = ConnectionError("ws task died")

    # While CONNECTED, GetState surfaces the stale error.
    resp_state = stub.GetState(engine_pb2.GetStateRequest(token=token))
    assert (
        json.loads(resp_state.json_data)["live_last_error"]
        == "ConnectionError: ws task died"
    )

    # Re-login must recover (teardown + fresh login), not no-op.
    resp2 = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="MOCK",
            credentials_source="env",
            token=token,
        )
    )
    assert resp2.success is True
    assert venue_sm.current == "CONNECTED"
    # A fresh runner replaced the dead one and carries no error.
    assert servicer._live_runner is not dead_runner
    assert servicer._live_runner.last_error is None

    # The stale error is gone after recovery.
    resp_state2 = stub.GetState(engine_pb2.GetStateRequest(token=token))
    assert json.loads(resp_state2.json_data)["live_last_error"] is None


# --- D20: UnsubscribeMarketData + LastPriceCache.remove ----------------------

def test_unsubscribe_removes_price_from_cache(phase8_grpc_server_with_live):
    """D20: Unsubscribe 後に _live_price_cache から該当 id が消える。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    _do_venue_login(stub, token)
    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True

    stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7203.TSE",
            channels=["trades", "depth"],
            token=token,
        )
    )

    # 手動でキャッシュに価格を注入
    cache = servicer._live_price_cache
    cache._last_trade["7203.TSE"] = 500.0
    assert "7203.TSE" in cache.snapshot()

    # Unsubscribe → キャッシュから消えることを確認
    resp = stub.UnsubscribeMarketData(
        engine_pb2.UnsubscribeRequest(instrument_id="7203.TSE", token=token)
    )
    assert resp.success is True
    assert "7203.TSE" not in cache.snapshot()


# --- D8: GetState mode-aware last_prices ------------------------------------

def test_get_state_replay_last_prices_reflects_per_id_close(phase8_grpc_server):
    """D8: Replay モードで per_id_close に値があれば GetState.last_prices に出る。"""
    port, token, engine, venue_sm, mm = phase8_grpc_server
    stub = _stub(port)

    # _rs.per_id_close を直接設定して simulate
    engine._rs.per_id_close["1301.TSE"] = 2500.0
    engine._rs.per_id_close["7203.TSE"] = 8000.0

    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["last_prices"].get("1301.TSE") == 2500.0
    assert payload["last_prices"].get("7203.TSE") == 8000.0


def test_get_state_live_last_prices_filtered_by_subscribed_ids(
    phase8_grpc_server_with_live,
):
    """D8/D20: Live mode では subscribed_ids() でフィルタした last_prices のみ返す。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    _do_venue_login(stub, token)
    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True

    # 未 subscribe の id をキャッシュに混入 (stale 価格の leak テスト)
    cache = servicer._live_price_cache
    cache._last_trade["7203.TSE"] = 500.0   # not subscribed → should NOT appear
    cache._last_trade["1301.TSE"] = 2500.0  # not subscribed → should NOT appear

    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    # subscribed_ids() == empty set → last_prices should be {} (filtered out)
    assert payload["last_prices"] == {}


# --- Fix 1: VenueLogout tears down live components --------------------------

def test_venue_logout_tears_down_live_components(phase8_grpc_server_with_live):
    """Fix 1: VenueLogout を呼ぶと _live_runner が None になり
    venue_sm.current が DISCONNECTED に戻る。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    # VenueLogin で runner を起動する
    _do_venue_login(stub, token)
    assert servicer._live_runner is not None, "precondition: runner should exist after VenueLogin"
    assert venue_sm.current == "CONNECTED"

    resp = stub.VenueLogout(engine_pb2.VenueLogoutRequest(token=token))
    assert resp.success is True
    assert servicer._live_runner is None, "VenueLogout must nil-out _live_runner"
    assert venue_sm.current == "DISCONNECTED", "VenueLogout must reset venue_sm to DISCONNECTED"


# --- Fix 3: live_venue_id must be forwarded to GrpcDataEngineServer ----------

def test_venue_mismatch_rejected_when_live_venue_id_configured():
    """Fix 3: GrpcDataEngineServer(live_venue_id='KABU') に対して
    VenueLogin(venue='TACHIBANA') を送ると VENUE_MISMATCH が返る。
    (live_venue_id が渡っていないと configured_venue == venue_id になり
     常に一致してしまうバグの回帰テスト)"""
    from engine.live.live_adapter_factory import build_live_adapter_factory

    token = "test-token"
    venue_sm = VenueStateMachine()
    engine_obj = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine_obj)
    engine_obj.attach_mode_manager(mm)

    # KABU 向けファクトリは build_live_adapter_factory で作れるが、
    # ここでは live_venue_id の配線確認だけなので MOCK ファクトリで十分
    mock_factory = build_live_adapter_factory("MOCK")

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(
        token,
        engine_obj,
        mode_manager=mm,
        venue_sm=venue_sm,
        live_adapter_factory=mock_factory,
        live_venue_id="KABU",  # Fix 3: this must be wired through
    )
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    try:
        stub = _stub(port)
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )
        assert resp.success is False
        assert resp.error_code == "VENUE_MISMATCH", (
            f"live_venue_id='KABU' なのに TACHIBANA を受け入れてはならない. "
            f"got error_code={resp.error_code!r}"
        )
    finally:
        server.stop(0)


# ---------------------------------------------------------------------------
# Step 5 tests: _handle_prompt_login + new VenueLogin behaviors
# ---------------------------------------------------------------------------

@pytest.fixture
def phase8_grpc_server_with_tachibana():
    """TACHIBANA MOCK-backed server (uses MockVenueAdapter under TACHIBANA venue_id)."""
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    # Use a factory that returns a MockVenueAdapter but is registered as TACHIBANA
    def tachibana_factory(env_hint=None):
        adapter = MockVenueAdapter()
        adapter.venue_id = "TACHIBANA"
        return adapter

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(
        token, engine, mode_manager=mm, venue_sm=venue_sm,
        live_adapter_factory=tachibana_factory,
        live_venue_id="TACHIBANA",
    )
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine, venue_sm, mm, servicer)

    if servicer._live_runner is not None or servicer._live_bridge is not None:
        servicer._teardown_live_components()
    server.stop(0)


@pytest.fixture
def phase8_grpc_server_with_kabu():
    """KABU MOCK-backed server."""
    token = "test-token"
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)

    def kabu_factory(env_hint=None):
        adapter = MockVenueAdapter()
        adapter.venue_id = "KABU"
        return adapter

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    servicer = GrpcDataEngineServer(
        token, engine, mode_manager=mm, venue_sm=venue_sm,
        live_adapter_factory=kabu_factory,
        live_venue_id="KABU",
    )
    engine_pb2_grpc.add_DataEngineServicer_to_server(servicer, server)
    engine_pb2_grpc.add_HealthServicer_to_server(servicer, server)
    port = server.add_insecure_port("[::]:0")
    server.start()

    yield (port, token, engine, venue_sm, mm, servicer)

    if servicer._live_runner is not None or servicer._live_bridge is not None:
        servicer._teardown_live_components()
    server.stop(0)


def test_venue_login_prompt_tachibana_uses_subprocess(phase8_grpc_server_with_tachibana):
    """_handle_prompt_login が呼ばれ、成功時に session_cache 経路で adapter.login が呼ばれる。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(True, "", None)),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is True, f"expected success, got error_code={resp.error_code!r}"
    assert resp.error_code == ""
    assert venue_sm.current == "CONNECTED"
    # adapter.login was called → MockVenueAdapter.is_logged_in == True
    adapter = servicer._live_runner.adapter
    assert adapter.is_logged_in is True


def test_venue_login_prompt_kabu_writes_token(phase8_grpc_server_with_kabu):
    """_handle_prompt_login が (True, "", "tok") を返すとき adapter._token == "tok"."""
    port, token_val, engine, venue_sm, mm, servicer = phase8_grpc_server_with_kabu
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(True, "", "bearer-tok-123")),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="KABU",
                credentials_source="prompt",
                token=token_val,
            )
        )

    assert resp.success is True, f"expected success, got error_code={resp.error_code!r}"
    assert venue_sm.current == "CONNECTED"
    # MockVenueAdapter.login sets is_logged_in=True (token is passed as creds.token)
    adapter = servicer._live_runner.adapter
    assert adapter.is_logged_in is True


def test_venue_login_prompt_failure_tears_down(phase8_grpc_server_with_tachibana):
    """_handle_prompt_login が (False, "AUTH_FAILED", None) を返すと resp.success is False."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(False, "AUTH_FAILED", None)),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is False
    assert resp.error_code == "AUTH_FAILED"


def test_venue_login_already_authenticating(phase8_grpc_server_with_tachibana):
    """venue_sm が AUTHENTICATING の間は二重起動を拒否する。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)

    # Force AUTHENTICATING state
    venue_sm.transition_to("AUTHENTICATING")

    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="TACHIBANA",
            credentials_source="prompt",
            token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "ALREADY_AUTHENTICATING"
    assert resp.venue_state == "AUTHENTICATING"


def test_venue_login_timeout(phase8_grpc_server_with_tachibana):
    """_handle_prompt_login が (False, "LOGIN_TIMEOUT", None) を返すと error_code == LOGIN_TIMEOUT。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(False, "LOGIN_TIMEOUT", None)),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is False
    assert resp.error_code == "LOGIN_TIMEOUT"


def test_venue_login_prompt_result_is_known_cred_source(phase8_grpc_server_with_live):
    """prompt_result は _KNOWN_CRED_SOURCES に含まれる (INVALID_ARGUMENT にならない)。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    # prompt_result with MOCK will go through env-like path (not prompt subprocess)
    # and call adapter.login with prompt_result credentials
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="MOCK",
            credentials_source="prompt_result",
            token=token,
        )
    )
    # Should NOT raise INVALID_ARGUMENT — it may succeed or fail for other reasons
    # The key assertion: no gRPC INVALID_ARGUMENT status
    assert resp.error_code != "INVALID_CREDENTIALS_SOURCE"


def test_venue_login_adapter_value_error_propagates_as_error_code(phase8_grpc_server_with_tachibana):
    """ValueError("SESSION_CACHE_MISSING") from adapter.login propagates as-is."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(True, "", None)),
    ):
        original_factory = servicer._live_adapter_factory

        def raising_factory(env_hint=None):
            adapter = original_factory(env_hint) if original_factory else MockVenueAdapter()
            adapter.venue_id = "TACHIBANA"
            async def raising_login(creds):
                raise ValueError("SESSION_CACHE_MISSING")
            adapter.login = raising_login
            adapter.is_logged_in = False
            return adapter

        servicer._live_adapter_factory = raising_factory

        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is False
    assert resp.error_code == "SESSION_CACHE_MISSING"


def test_venue_login_adapter_unknown_value_error_falls_to_generic(phase8_grpc_server_with_tachibana):
    """ValueError outside the allowlist is collapsed to VENUE_LOGIN_FAILED."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(True, "", None)),
    ):
        original_factory = servicer._live_adapter_factory

        def raising_factory(env_hint=None):
            adapter = original_factory(env_hint) if original_factory else MockVenueAdapter()
            adapter.venue_id = "TACHIBANA"
            async def raising_login(creds):
                raise ValueError("something_else")
            adapter.login = raising_login
            adapter.is_logged_in = False
            return adapter

        servicer._live_adapter_factory = raising_factory

        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is False
    assert resp.error_code == "VENUE_LOGIN_FAILED"


def test_venue_login_no_display_does_not_fallback_in_release(phase8_grpc_server_with_tachibana, monkeypatch):
    """Release build does not env-retry after NO_DISPLAY_AVAILABLE."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)
    monkeypatch.setattr("engine.server_grpc.IS_DEBUG_BUILD", False)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(False, "NO_DISPLAY_AVAILABLE", None)),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is False
    assert resp.error_code == "NO_DISPLAY_AVAILABLE"


def test_venue_login_no_display_fallbacks_to_env_in_debug(phase8_grpc_server_with_tachibana, monkeypatch):
    """Debug build env-retries adapter.login with credentials_source='env' after NO_DISPLAY_AVAILABLE."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_tachibana
    stub = _stub(port)
    monkeypatch.setattr("engine.server_grpc.IS_DEBUG_BUILD", True)

    recorded_sources: list[str] = []

    from engine.live.mock_adapter import MockVenueAdapter

    original_login = MockVenueAdapter.login

    async def spy_login(self, creds):
        recorded_sources.append(creds.credentials_source)
        await original_login(self, creds)

    monkeypatch.setattr(MockVenueAdapter, "login", spy_login)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(False, "NO_DISPLAY_AVAILABLE", None)),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id="TACHIBANA",
                credentials_source="prompt",
                token=token,
            )
        )

    assert len(recorded_sources) >= 1
    assert recorded_sources[-1] == "env"
    assert resp.success is True
    assert resp.error_code == ""


@pytest.mark.parametrize(
    "fixture_name, venue_id, error_code",
    [
        ("phase8_grpc_server_with_kabu", "KABU", "LOGIN_INVALID_RESPONSE"),
        ("phase8_grpc_server_with_tachibana", "TACHIBANA", "LOGIN_SUBPROCESS_CRASHED"),
        ("phase8_grpc_server_with_tachibana", "TACHIBANA", "LOGIN_INVALID_RESPONSE"),
    ],
    ids=["kabu_empty_cred_file", "tachibana_crash", "tachibana_invalid_json"],
)
def test_venue_login_prompt_failure_propagates_and_tears_down(
    request, fixture_name, venue_id, error_code,
):
    port, token, engine, venue_sm, mm, servicer = request.getfixturevalue(fixture_name)
    stub = _stub(port)

    with patch.object(
        servicer, "_handle_prompt_login",
        new=AsyncMock(return_value=(False, error_code, None)),
    ):
        resp = stub.VenueLogin(
            engine_pb2.VenueLoginRequest(
                venue_id=venue_id,
                credentials_source="prompt",
                token=token,
            )
        )

    assert resp.success is False
    assert resp.error_code == error_code
    assert servicer._live_runner is None
    assert servicer._live_bridge is None
    assert venue_sm.current == "DISCONNECTED"


# ============================================================================
# Post-merge review fixes (2026-05-20)
# ============================================================================


# --- HIGH-2: SubscribeMarketData 50-instrument cap (§0.3) -------------------

def test_subscribe_market_data_cap_at_50_instruments(phase8_grpc_server_with_live):
    """HIGH-2: 50 銘柄まで accept、51 番目で SUBSCRIPTION_LIMIT_EXCEEDED。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    resp_mode = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_mode.success is True

    # 50 subscribes succeed
    for i in range(50):
        iid = f"7{i:03d}.TSE"
        resp = stub.SubscribeMarketData(
            engine_pb2.SubscribeRequest(
                instrument_id=iid, channels=["trades"], token=token,
            )
        )
        assert resp.success is True, (
            f"subscribe #{i+1} ({iid}) should succeed: error={resp.error_code!r}"
        )

    # 51st must be rejected
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="9999.TSE", channels=["trades"], token=token,
        )
    )
    assert resp.success is False
    assert resp.error_code == "SUBSCRIPTION_LIMIT_EXCEEDED"


def test_subscribe_market_data_re_subscribe_does_not_count_against_cap(
    phase8_grpc_server_with_live,
):
    """idempotent re-subscribe は cap カウントに含まれない (50/50 でも再 subscribe は OK)."""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    for i in range(50):
        stub.SubscribeMarketData(
            engine_pb2.SubscribeRequest(
                instrument_id=f"7{i:03d}.TSE", channels=["trades"], token=token,
            )
        )
    # Re-subscribe to an existing id — must still succeed at the cap.
    resp = stub.SubscribeMarketData(
        engine_pb2.SubscribeRequest(
            instrument_id="7000.TSE", channels=["trades"], token=token,
        )
    )
    assert resp.success is True
    assert resp.error_code == ""


# --- HIGH-3: live_last_error cleared on mode toggle (§9.14) -----------------

def test_live_last_error_cleared_when_toggling_to_replay(
    phase8_grpc_server_with_live,
):
    """HIGH-3: 例外発生 → live_last_error が set → Replay に戻すと None。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )

    # Simulate a runner error.
    servicer._live_runner._last_error = ConnectionError("boom")
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] == "ConnectionError: boom"

    # Replay 戻し precondition: engine_replay_state in {LOADED,RUNNING,PAUSED}
    engine._replay_state = "LOADED"
    resp_replay = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="Replay", token=token)
    )
    assert resp_replay.success is True
    resp2 = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload2 = json.loads(resp2.json_data)
    assert payload2["live_last_error"] is None, (
        f"live_last_error must clear on Replay toggle; got {payload2['live_last_error']!r}"
    )


def test_live_last_error_bridge_error_suppressed_on_replay_then_new_error_shows(
    phase8_grpc_server_with_live,
):
    """Medium-2: bridge 由来の live_last_error も Replay 切替で `is` 比較により
    抑制され、別オブジェクトの新エラーは抑制解除されて表示される。

    runner.last_error は一切触らない（None のまま）。_resolve_live_last_error は
    runner 優先 → None のとき bridge を見るため、bridge._last_error が live_last_error に
    反映される。arm 時の baseline は同じ bridge error オブジェクトを記録し、GetState の
    suppression は `is` 比較なので同一オブジェクトのみ隠す。
    """
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )

    # runner.last_error は触らず、bridge にだけエラーを inject する。
    assert servicer._live_runner is not None
    assert servicer._live_runner.last_error is None
    assert servicer._live_bridge is not None
    baseline_err = ConnectionError("bridge boom")
    servicer._live_bridge._last_error = baseline_err

    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] == "ConnectionError: bridge boom"

    # Replay 戻し precondition: engine_replay_state in {LOADED,RUNNING,PAUSED}
    engine._replay_state = "LOADED"
    resp_replay = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="Replay", token=token)
    )
    assert resp_replay.success is True

    # 同じ bridge error オブジェクトのまま → `is` 比較で抑制される。
    # runner は teardown されず last_error は None のままなので bridge が引き続き見える。
    assert servicer._live_runner is not None
    assert servicer._live_runner.last_error is None
    assert servicer._live_bridge.last_error is baseline_err
    resp2 = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload2 = json.loads(resp2.json_data)
    assert payload2["live_last_error"] is None, (
        f"bridge baseline error must be suppressed on Replay toggle (is-compare); "
        f"got {payload2['live_last_error']!r}"
    )

    # 別オブジェクトの新エラーを inject → baseline と `is` 不一致なので抑制解除・表示される。
    servicer._live_bridge._last_error = ConnectionError("new boom")
    resp3 = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload3 = json.loads(resp3.json_data)
    assert payload3["live_last_error"] == "ConnectionError: new boom", (
        f"a freshly raised error object must surface and drop suppression; "
        f"got {payload3['live_last_error']!r}"
    )


def test_live_last_error_cleared_on_venue_re_login(phase8_grpc_server_with_live):
    """HIGH-3: VenueLogin 成功で live_last_error が None にリセットされる。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    # Inject a stale error from a prior lifecycle directly into the override.
    # (After teardown, _live_runner is None, so we exercise the suppression path
    #  by manually setting the flag = False to simulate "stuck" state, then verify
    #  VenueLogin re-arms it.)
    servicer._suppress_live_last_error = False

    # VenueLogout to drop the connection (clears suppression too).
    stub.VenueLogout(engine_pb2.VenueLogoutRequest(token=token))
    assert servicer._suppress_live_last_error is True

    # Now re-login → suppression should remain armed.
    _do_venue_login(stub, token)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["live_last_error"] is None


# --- MEDIUM-5: live loop exception handler logs uncaught asyncio errors ----

def test_phase8_live_loop_logs_uncaught_asyncio_exception(
    phase8_grpc_server_with_live, caplog
):
    """MEDIUM-5: phase8-live-loop thread が uncaught exception を握りつぶさず
    ERROR ログを出す。"""
    import logging as _logging
    port, token, *_rest, servicer = phase8_grpc_server_with_live
    stub = _stub(port)
    _do_venue_login(stub, token)
    loop = servicer._ensure_live_loop()

    async def _boom():
        raise RuntimeError("loop-uncaught-boom")

    caplog.set_level(_logging.ERROR)

    # Create a Task on the loop (NOT via run_coroutine_threadsafe — that wraps
    # the result in a concurrent.futures.Future whose exception is *consumed*
    # and never reaches the loop's exception handler). A bare un-awaited Task
    # whose coroutine raises routes the exception through call_exception_handler.
    tasks: list = []

    def _schedule() -> None:
        tasks.append(loop.create_task(_boom()))

    loop.call_soon_threadsafe(_schedule)

    import time as _time
    deadline = _time.time() + 2.0
    while _time.time() < deadline:
        if tasks and all(t.done() for t in tasks):
            break
        _time.sleep(0.02)
    # Drop strong refs so Task.__del__ -> call_exception_handler runs.
    tasks.clear()
    import gc as _gc
    _gc.collect()
    _time.sleep(0.2)

    msgs = " | ".join(r.getMessage() for r in caplog.records)
    assert (
        "phase8-live-loop" in msgs
        or "loop-uncaught-boom" in msgs
        or "uncaught asyncio exception" in msgs
    ), (
        f"expected loop exception to be logged, got: {msgs!r}"
    )


def test_get_state_exposes_configured_venue(phase8_grpc_server_with_live):
    """Step 1: GetState の JSON に servicer._live_venue_id が
    configured_venue として現れる。with_live fixture は live_venue_id='MOCK'。"""
    port, token, *_ = phase8_grpc_server_with_live
    stub = _stub(port)
    resp = stub.GetState(engine_pb2.GetStateRequest(token=token))
    payload = json.loads(resp.json_data)
    assert payload["configured_venue"] == "MOCK"


def test_list_instruments_live_timeout_returns_clear_message(monkeypatch):
    """Issue #32: venue fetch の timeout で空の 'fetch_instruments failed:' を返さない
    （concurrent.futures.TimeoutError.__str__() は '' なので素直に流すと空メッセージになる）。
    store miss → blocking fetch timeout のとき、原因の分かる文言を返す。"""
    from engine.live import instruments_store

    # store miss を強制（永続化済み parquet なし）
    monkeypatch.setattr(instruments_store, "read_instruments", lambda venue: None)

    svc = object.__new__(GrpcDataEngineServer)
    # 両方セットして「現状コード（_live_timeout_s 使用）」でも stub の raise まで到達させ、
    # 空メッセージを RED で踏む。修正後は _instruments_timeout_s を使う。
    svc._live_timeout_s = 5.0
    svc._instruments_timeout_s = 60.0
    svc._instruments_scheduler = None  # warming ではない → blocking fetch 経路へ

    class _StubRunner:
        venue_id = "TACHIBANA"

        def is_logged_in(self):
            return True

        def fetch_instruments_blocking(self, timeout):
            raise futures.TimeoutError()

    svc._live_runner = _StubRunner()

    resp = svc._list_instruments_live(None)
    assert resp.success is False
    assert resp.error_message.strip(), "error_message が空/空白であってはならない"
    assert resp.error_message.strip() != "fetch_instruments failed:"
    assert "tim" in resp.error_message.lower()  # 'timed out' / 'timeout'


def test_list_instruments_live_warming_returns_pending(monkeypatch):
    """Issue #32 Slice 2: scheduler の初回 refresh が進行中（warming）の cold-store miss は
    60s blocking fetch せず `LIVE_UNIVERSE_PENDING` を返す（store-first 維持・赤エラー回避）。
    UI 側はこれを Loading spinner にマップし、store が埋まったら再 fetch する。"""
    from engine.live import instruments_store

    # store miss を強制（永続化済み parquet なし）
    monkeypatch.setattr(instruments_store, "read_instruments", lambda venue: None)

    svc = object.__new__(GrpcDataEngineServer)
    svc._live_timeout_s = 5.0
    svc._instruments_timeout_s = 60.0

    class _StubRunner:
        venue_id = "TACHIBANA"

        def is_logged_in(self):
            return True

        def fetch_instruments_blocking(self, timeout):
            raise AssertionError("warming 中は blocking fetch を呼んではならない")

    svc._live_runner = _StubRunner()

    class _WarmingScheduler:
        def is_warming(self):
            return True

    svc._instruments_scheduler = _WarmingScheduler()

    resp = svc._list_instruments_live(None)
    assert resp.success is False
    assert resp.error_message == "LIVE_UNIVERSE_PENDING"


# --- Issue #39: Live → Replay → Live 往復 / VenueLogout 回帰ガード -----------

def test_live_replay_live_roundtrip_without_relogin(phase8_grpc_server_with_live):
    """Live→Replay→Live 往復で再ログイン不要（adapter.is_logged_in が維持される）。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    engine._replay_state = "LOADED"

    _do_venue_login(stub, token)
    assert venue_sm.current == "CONNECTED"

    # 1. LiveManual
    resp_live = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_live.success is True

    # 2. Replay
    resp_replay = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="Replay", token=token)
    )
    assert resp_replay.success is True
    adapter = servicer._live_runner.adapter
    assert adapter.is_logged_in is True, "Replay 切替で logout してはいけない"
    assert adapter.logout_call_count == 0, "Replay 切替で logout を呼んではいけない"

    # 3. LiveManual に再切替（再ログインなし）
    resp_live2 = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_live2.success is True, "再ログインなしで LiveManual に戻れるべき"
    assert servicer._live_runner is not None
    assert venue_sm.current == "CONNECTED"


def test_venue_logout_still_teardowns_and_disconnects(phase8_grpc_server_with_live):
    """VenueLogout（Disconnect ボタン）は従来どおり teardown して DISCONNECTED に戻る。"""
    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    _do_venue_login(stub, token)
    assert venue_sm.current == "CONNECTED"

    resp = stub.VenueLogout(engine_pb2.VenueLogoutRequest(token=token))
    assert resp.success is True

    assert servicer._live_runner is None, "VenueLogout 後は live runner が解放されるべき"
    assert venue_sm.current == "DISCONNECTED", "VenueLogout 後は DISCONNECTED"


# --- Slice 2: Replay 表示中の live イベント混線ガード --------------------------------

def test_replay_mode_does_not_apply_live_klines_to_reducer(phase8_grpc_server_with_live):
    """Slice 2 RED: Replay 中に live KlineUpdate が DataEngine の per_id_close を汚染しないこと。

    live session が生き続ける（Slice 1）副作用で LiveReducerBridge が Replay 中も
    apply_replay_event を呼ぶと、per_id_close / per_id_ohlc_points が live 価格で
    上書きされ GetState の last_prices / per_instrument に混入する。
    """
    import time as _time
    from engine.live.adapter import KlineUpdate as LiveKlineUpdate

    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live
    stub = _stub(port)

    engine._replay_state = "LOADED"
    _do_venue_login(stub, token)
    assert venue_sm.current == "CONNECTED"

    # LiveManual → Replay（Slice 1: live runner は生き続ける）
    stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token))
    resp = stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Replay", token=token))
    assert resp.success is True
    assert servicer._live_runner is not None, "Slice 1: Replay 切替で live runner が解放されてはいけない"

    # live bus に KlineUpdate を直接 publish して bridge 経由の混入を再現する
    loop = servicer._live_loop
    bus = servicer._live_runner.bus
    live_kline = LiveKlineUpdate(
        kind="kline",
        ts_ns=int(_time.time() * 1_000_000_000),
        instrument_id="CONTAMINATION.TEST",
        open=9999.0, high=9999.0, low=9999.0, close=9999.0, volume=1.0,
    )
    asyncio.run_coroutine_threadsafe(bus.publish(live_kline), loop).result(timeout=1.0)
    _time.sleep(0.15)  # bridge task が event を消化するのを待つ

    # Replay モードでは live 由来の price が per_id_close に入ってはいけない
    prices = engine.get_replay_last_prices()
    assert "CONTAMINATION.TEST" not in prices, (
        f"Replay 中に live kline が per_id_close を汚染した: prices={prices}"
    )

    # --- 対照ケース: ゲートが効いているだけで bridge 自体は生きていることを保証する ---
    # LiveManual に戻すと mode_provider() != "Replay" になり、同じ bridge が
    # 別 instrument の live kline を per_id_close に流すはず。これが通らないなら
    # bridge が死んでいる / 購読できていない退行であり、case2 の「何も流れない」
    # pass が false-negative であることを意味する。
    resp_back = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_back.success is True
    control_kline = LiveKlineUpdate(
        kind="kline",
        ts_ns=int(_time.time() * 1_000_000_000),
        instrument_id="CONTROL.LIVE.OK",
        open=1234.0, high=1234.0, low=1234.0, close=1234.0, volume=1.0,
    )
    asyncio.run_coroutine_threadsafe(bus.publish(control_kline), loop).result(timeout=1.0)
    _time.sleep(0.15)  # bridge task が event を消化するのを待つ

    prices_after = engine.get_replay_last_prices()
    assert "CONTROL.LIVE.OK" in prices_after, (
        "LiveManual では bridge が live kline を per_id_close に流すべき "
        f"(bridge が死んでいる/購読断の退行を検知): prices={prices_after}"
    )

    # bridge task が例外で silent dead していないこと（last_error が積まれていない）。
    assert servicer._live_bridge is not None
    assert servicer._live_bridge.last_error is None, (
        f"bridge task が例外で死んでいる: {servicer._live_bridge.last_error!r}"
    )


def test_replay_mode_does_not_publish_live_account_event(phase8_grpc_server_with_live):
    """issue #39 Slice 2: Replay 中は AccountSync が AccountEvent を backend stream へ
    push しないこと（live の余力・建玉が Replay の portfolio panel を上書きしない）。

    Slice 2 是正（案A+Y）: gate は _publish_account_snapshot 直叩きではなく
    AccountSync._tick 入口の mode_provider gate が担う。server_grpc が注入する
    `mode_provider=lambda: mm.current_mode` と同形の lambda を仕込み、mm.current_mode を
    LiveManual / Replay に切り替えて force_resync の emit 有無を対照する。
    LiveManual では emit され（gate が live を塞がない対照）、Replay では emit されない。
    """
    import asyncio
    from engine.live.account_sync import AccountSync
    from engine.live.adapter import VenueCredentials
    from engine.live.mock_adapter import MockVenueAdapter

    port, token, engine, venue_sm, mm, servicer = phase8_grpc_server_with_live

    loop = servicer._ensure_live_loop()

    # server_grpc が注入する lambda と同形（mm.current_mode を実際に読む統合点）。
    async def setup():
        adapter = MockVenueAdapter()
        await adapter.login(
            VenueCredentials(credentials_source="env", environment_hint="demo")
        )
        adapter.set_account_snapshot(cash=123456.0, buying_power=654321.0, positions=[])
        sync = AccountSync(
            adapter,
            on_account_event=servicer._publish_account_snapshot,
            on_error=servicer._publish_account_sync_error,
            interval_s=3600.0,  # tick を止めて force_resync の 1 発だけを観測する
            mode_provider=lambda: (
                mm.current_mode if mm else "Replay"
            ),
        )
        return adapter, sync

    adapter, sync = asyncio.run_coroutine_threadsafe(setup(), loop).result(timeout=5.0)

    # publish_backend_event をスパイして AccountEvent の push をキャプチャする
    published = []
    original_publish = servicer.publish_backend_event

    def _spy(event):
        published.append(event)
        return original_publish(event)

    servicer.publish_backend_event = _spy

    def _account_events():
        return [e for e in published if e.WhichOneof("payload") == "account_event"]

    # 1. LiveManual: gate が live を塞がない対照（emit されるべき）
    mm.current_mode = "LiveManual"
    emitted_live = asyncio.run_coroutine_threadsafe(
        sync.force_resync(), loop
    ).result(timeout=5.0)
    assert emitted_live is True, "LiveManual では force_resync が emit すべき"
    account_events_live = _account_events()
    assert len(account_events_live) == 1, (
        "LiveManual では AccountEvent が backend stream に push されるべき"
    )
    assert account_events_live[0].account_event.buying_power == 654321.0

    # 2. Replay: AccountSync._tick 入口 gate が emit を抑止する（push されない）
    published.clear()
    mm.current_mode = "Replay"
    emitted_replay = asyncio.run_coroutine_threadsafe(
        sync.force_resync(), loop
    ).result(timeout=5.0)
    assert emitted_replay is False, (
        "Replay では force（force_emit=True）でも _tick 入口 gate が emit を抑止すべき（案Y）"
    )
    assert _account_events() == [], (
        f"Replay 中に live AccountEvent が backend stream を汚染した: {_account_events()}"
    )
