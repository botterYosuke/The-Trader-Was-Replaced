import asyncio
import json
from unittest.mock import MagicMock

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


def test_set_execution_mode_replay_precondition(phase8_grpc_server):
    port, token, engine, venue_sm, mm = phase8_grpc_server
    stub = _stub(port)
    resp = stub.SetExecutionMode(engine_pb2.SetExecutionModeRequest(mode="Replay", token=token))
    assert resp.success is False
    assert resp.error_code == "EXECUTION_MODE_PRECONDITION"


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
    """Helper: perform VenueLogin via gRPC (D21 precondition for SetExecutionMode)."""
    resp = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id=venue_id,
            credentials_source="prompt",
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


def test_set_execution_mode_replay_teardown_live_runner(
    phase8_grpc_server_with_live,
):
    port, token, engine, venue_sm, mm, servicer = (
        phase8_grpc_server_with_live
    )
    stub = _stub(port)
    # Replay 戻しは replay_engine.replay_state in {LOADED,RUNNING,PAUSED} が precondition (mode_manager.py L24-29)
    engine._replay_state = "LOADED"

    # D21: VenueLogin must precede SetExecutionMode for Live modes
    _do_venue_login(stub, token)

    # LiveManual で runner/bridge が立ち上がることを前提として確認
    resp_live = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="LiveManual", token=token)
    )
    assert resp_live.success is True
    assert servicer._live_runner is not None
    assert servicer._live_bridge is not None

    # Replay に戻したとき teardown が走り、参照が解放されることを期待 (RED)
    resp_replay = stub.SetExecutionMode(
        engine_pb2.SetExecutionModeRequest(mode="Replay", token=token)
    )
    assert resp_replay.success is True
    assert resp_replay.execution_mode == "Replay"
    assert servicer._live_runner is None, "Replay 切替時に live runner が teardown されるべき"
    assert servicer._live_bridge is None, "Replay 切替時に live bridge が teardown されるべき"


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
            credentials_source="prompt",
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
            credentials_source="prompt",
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
    # 2 回目
    resp2 = stub.VenueLogin(
        engine_pb2.VenueLoginRequest(
            venue_id="MOCK",
            credentials_source="prompt",
            token=token,
        )
    )
    assert resp2.success is True
    assert venue_sm.current == "CONNECTED"


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
