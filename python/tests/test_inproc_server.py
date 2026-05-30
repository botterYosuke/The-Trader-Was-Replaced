"""Tests for InprocLiveServer — Phase 4 / issue #64.

Verifies that InprocLiveServer correctly wraps GrpcDataEngineServer and routes
each live command to the corresponding Python method without gRPC overhead.
"""
from pathlib import Path

import pytest

from engine.core import DataEngine


# ---------------------------------------------------------------------------
# Smoke: InprocLiveServer imports and instantiates without a live venue
# ---------------------------------------------------------------------------

def test_inproc_live_server_imports():
    from engine.inproc_server import InprocLiveServer  # noqa: F401


def test_inproc_live_server_instantiates_without_live_venue():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    assert srv is not None


def test_inproc_live_server_get_state_json_returns_valid_json():
    import json
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    json_str = srv.get_state_json()
    state = json.loads(json_str)
    assert isinstance(state, dict)
    # replay_state should default to IDLE
    assert state.get("replay_state") == "IDLE"


# ---------------------------------------------------------------------------
# set_execution_mode
# ---------------------------------------------------------------------------

def test_set_execution_mode_replay():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.set_execution_mode("Replay")
    assert isinstance(result, dict)
    assert "success" in result
    assert "error_code" in result


# ---------------------------------------------------------------------------
# venue_login without a configured live adapter → LIVE_ADAPTER_NOT_CONFIGURED
# ---------------------------------------------------------------------------

def test_venue_login_no_adapter_returns_not_configured():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.venue_login("MOCK", "prompt", None)
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "LIVE_ADAPTER_NOT_CONFIGURED"


# ---------------------------------------------------------------------------
# venue_logout — no adapter → graceful (no crash)
# ---------------------------------------------------------------------------

def test_venue_logout_no_adapter():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.venue_logout()
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# list_instruments local — catalog not set → success=False or empty list
# ---------------------------------------------------------------------------

def test_list_instruments_local_no_catalog():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.list_instruments("local")
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# list_all_listed_symbols — no catalog → success=False
# ---------------------------------------------------------------------------

def test_list_all_listed_symbols_no_catalog():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.list_all_listed_symbols("2024-01-01")
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# subscribe / unsubscribe market data — no adapter → error or graceful
# ---------------------------------------------------------------------------

def test_subscribe_market_data_no_adapter():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.subscribe_market_data("7203.TSE")
    assert isinstance(result, dict)
    assert "success" in result


def test_unsubscribe_market_data_no_adapter():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.unsubscribe_market_data("7203.TSE")
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# get_orders — no session → returns empty orders list
# ---------------------------------------------------------------------------

def test_get_orders_no_session():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.get_orders("MOCK")
    assert isinstance(result, dict)
    assert "orders" in result
    assert isinstance(result["orders"], list)


# ---------------------------------------------------------------------------
# force_account_snapshot — no session → graceful error or accepted
# ---------------------------------------------------------------------------

def test_force_account_snapshot_no_session():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.force_account_snapshot()
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# submit_secret — no vault / no pending request → graceful
# ---------------------------------------------------------------------------

def test_submit_secret_no_pending():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.submit_secret("req-none", "secret")
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# get_portfolio — before any run → returns empty portfolio
# ---------------------------------------------------------------------------

def test_get_portfolio_before_run():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.get_portfolio()
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# live strategy operations — no session → LIVE_VENUE_NOT_LOGGED_IN / LIVE_ADAPTER_NOT_CONFIGURED
# ---------------------------------------------------------------------------

def test_start_live_strategy_no_adapter():
    """register は venue 非依存で strategy_id を発行する。venue precondition は
    start_live_strategy 側で効く（EXECUTION_MODE_PRECONDITION）."""
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    strategy_file = Path(__file__).parents[2] / "examples/test_strategy_minute.py"
    result = srv.register_live_strategy(str(strategy_file))
    assert isinstance(result, dict)
    assert result["success"] is True
    assert result["strategy_id"]

    start = srv.start_live_strategy(result["strategy_id"], "7203.TSE", "MOCK")
    assert isinstance(start, dict)
    assert start["success"] is False


def test_stop_live_strategy_no_run():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.stop_live_strategy("run-none")
    assert isinstance(result, dict)
    assert "success" in result


def test_pause_live_strategy_no_run():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.pause_live_strategy("run-none")
    assert isinstance(result, dict)
    assert "success" in result


def test_resume_live_strategy_no_run():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.resume_live_strategy("run-none")
    assert isinstance(result, dict)
    assert "success" in result


# ---------------------------------------------------------------------------
# place/cancel/modify order — no session → graceful error
# ---------------------------------------------------------------------------

def test_place_order_no_session():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.place_order(
        venue="MOCK",
        instrument_id="7203.TSE",
        side="BUY",
        qty=100.0,
        price=1000.0,
        order_type="LIMIT",
        time_in_force="DAY",
        second_secret=None,
    )
    assert isinstance(result, dict)
    assert result["success"] is False


def test_cancel_order_no_session():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.cancel_order(venue="MOCK", order_id="ord-1", second_secret=None)
    assert isinstance(result, dict)
    assert result["success"] is False


def test_modify_order_no_session():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    result = srv.modify_order(
        venue="MOCK",
        client_order_id="ord-1",
        new_qty=200.0,
        new_price=1100.0,
        second_secret=None,
    )
    assert isinstance(result, dict)
    assert result["success"] is False


# ---------------------------------------------------------------------------
# RED (#64-1): _parse_granularity_int は Rust から渡る proto int を受け取れる
#   Rust backend_transport.rs は granularity を proto enum の int (DAILY=3 等) で
#   cfg.set_item するが、現行 parser は文字列名前提のため int が常に TICK(0) に潰れる。
#   RED＝回帰ガード・fix は #64 後に green
# ---------------------------------------------------------------------------

def test_parse_granularity_int_accepts_proto_int_daily():
    from engine.backend_service import _parse_granularity_int
    from engine.proto import engine_pb2

    # Rust 側は proto enum の int をそのまま渡す（DAILY == 3）
    assert _parse_granularity_int(engine_pb2.DAILY) == engine_pb2.DAILY


def test_parse_granularity_int_accepts_proto_int_minute():
    from engine.backend_service import _parse_granularity_int
    from engine.proto import engine_pb2

    assert _parse_granularity_int(engine_pb2.MINUTE) == engine_pb2.MINUTE


# ---------------------------------------------------------------------------
# RED (#64-4): 配下ハンドラが RuntimeError 非サブクラス例外（concurrent.futures.TimeoutError）
#   を投げたとき、現行 place_order は `except RuntimeError` をすり抜けて PyO3 境界へ
#   例外を伝播させてしまう。修正後は except Exception で捕捉し error_code='INPROC_ERROR'
#   の dict を返すべき。
#   RED＝回帰ガード・fix は #64 後に green
# ---------------------------------------------------------------------------

def test_place_order_non_runtime_exception_returns_inproc_error():
    import concurrent.futures
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _raise_timeout(req, ctx):
        raise concurrent.futures.TimeoutError("inproc timeout")

    srv._svc._srv.PlaceOrder = _raise_timeout

    # 現状: Timeout 例外が except RuntimeError をすり抜けて送出される（RED）。
    # 修正後: dict が返り error_code == 'INPROC_ERROR' になる（GREEN）。
    result = srv.place_order(
        venue="MOCK",
        instrument_id="7203.TSE",
        side="BUY",
        qty=100.0,
        price=1000.0,
        order_type="LIMIT",
        time_in_force="DAY",
        second_secret=None,
    )
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ERROR"


# ---------------------------------------------------------------------------
# RED (#64-6): InprocLiveServer.close() は配下 GrpcDataEngineServer の live
#   コンポーネント teardown を呼ぶべき。InProc worker は command channel close
#   時に façade を drop するだけで、配下の live loop thread / runner / account
#   sync を停止しない（server_grpc.py の _live_loop/_live_thread は誰も止めない）。
#   修正後は close() が self._srv._teardown_live_components() を呼ぶ。
#   RED＝回帰ガード・fix は #64 後に green
# ---------------------------------------------------------------------------

def test_close_invokes_underlying_teardown():
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    calls = []

    def _spy_teardown():
        calls.append("teardown")

    srv._svc._srv._teardown_live_components = _spy_teardown

    # 現状: close() は no-op shell なので teardown を呼ばない（RED）。
    # 修正後: close() が self._srv._teardown_live_components() を呼ぶ（GREEN）。
    srv.close()

    assert calls == ["teardown"]


# ---------------------------------------------------------------------------
# RED (#64-フォロー①): 残り 15 メソッドの非 RuntimeError 例外が INPROC_ERROR を返す
#   現行: except RuntimeError のみで TimeoutError 等が PyO3 境界へ伝播する
#   修正後: except Exception (INPROC_ERROR) が捕捉し dict を返す
# ---------------------------------------------------------------------------

def test_venue_logout_non_runtime_exception_returns_inproc_error():
    import concurrent.futures
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _raise(req, ctx):
        raise concurrent.futures.TimeoutError("timeout")

    srv._svc._srv.VenueLogout = _raise
    result = srv.venue_logout()
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ERROR"


def test_list_instruments_non_runtime_exception_returns_inproc_error():
    import concurrent.futures
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _raise(req, ctx):
        raise concurrent.futures.TimeoutError("timeout")

    srv._svc._srv.ListInstruments = _raise
    result = srv.list_instruments("local")
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ERROR"
    assert isinstance(result.get("instruments"), list)
    assert isinstance(result.get("instrument_ids"), list)


def test_get_orders_non_runtime_exception_returns_inproc_error():
    import concurrent.futures
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _raise(req, ctx):
        raise concurrent.futures.TimeoutError("timeout")

    srv._svc._srv.GetOrders = _raise
    result = srv.get_orders("MOCK")
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ERROR"
    assert isinstance(result.get("orders"), list)


def test_start_engine_non_runtime_exception_returns_inproc_error():
    import concurrent.futures
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _raise(req, ctx):
        raise concurrent.futures.TimeoutError("timeout")

    srv._svc._srv.StartEngine = _raise
    result = srv.start_engine({"instrument_id": "7203.TSE", "strategy_file": "strat.py"})
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ERROR"


def test_get_portfolio_non_runtime_exception_returns_inproc_error():
    import concurrent.futures
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _raise(req, ctx):
        raise concurrent.futures.TimeoutError("timeout")

    srv._svc._srv.GetPortfolio = _raise
    result = srv.get_portfolio()
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ERROR"


def test_venue_logout_abort_still_returns_inproc_abort():
    """RuntimeError（NullContext.abort）は INPROC_ABORT のまま（回帰ガード）."""
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    def _abort(req, ctx):
        raise RuntimeError("abort!")

    srv._svc._srv.VenueLogout = _abort
    result = srv.venue_logout()
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error_code"] == "INPROC_ABORT"
