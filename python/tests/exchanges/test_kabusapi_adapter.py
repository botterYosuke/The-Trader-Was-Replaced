"""Tests for KabuStationAdapter skeleton (Phase 8 §1.3 / §3.2 B1)."""

import asyncio

import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.kabusapi import KabuStationAdapter
from engine.exchanges.kabusapi_url import endpoint
from engine.live.adapter import DepthUpdate, LiveVenueAdapter, TradesUpdate, VenueCredentials


def test_venue_id_is_kabu():
    assert KabuStationAdapter().venue_id == "KABU"


def test_protocol_compliance():
    assert isinstance(KabuStationAdapter(), LiveVenueAdapter)


def test_default_environment_is_verify():
    assert KabuStationAdapter()._env == "verify"


def test_environment_verify_accepted():
    assert KabuStationAdapter(environment="verify")._env == "verify"


def test_environment_prod_accepted():
    assert KabuStationAdapter(environment="prod")._env == "prod"


def test_invalid_environment_raises():
    with pytest.raises(ValueError):
        KabuStationAdapter(environment="staging")  # type: ignore[arg-type]


def test_login_session_cache_rejected():
    """kabu does not support session_cache credentials source (skill ADR)"""
    creds = VenueCredentials(credentials_source="session_cache")
    with pytest.raises(ValueError, match="UNSUPPORTED_FOR_VENUE"):
        asyncio.run(KabuStationAdapter().login(creds))


def test_login_prompt_raises_not_implemented():
    creds = VenueCredentials(credentials_source="prompt")
    with pytest.raises(NotImplementedError):
        asyncio.run(KabuStationAdapter().login(creds))


def test_logout_clears_token():
    a = KabuStationAdapter()
    a._token = "abc"
    asyncio.run(a.logout())
    assert a._token is None


# ---------------------------------------------------------------------------
# Phase 8 §3.2 Step 0: is_logged_in property
# ---------------------------------------------------------------------------


def test_is_logged_in_false_when_no_token():
    adapter = KabuStationAdapter()
    assert adapter.is_logged_in is False


def test_is_logged_in_true_when_token_set():
    adapter = KabuStationAdapter()
    adapter._token = "x"
    assert adapter.is_logged_in is True


# ---------------------------------------------------------------------------
# Phase 8 §3.2 Step 0: login(prompt_result)
# ---------------------------------------------------------------------------


async def test_login_prompt_result_success():
    """prompt_result + token="tok" → _token が設定される。"""
    adapter = KabuStationAdapter()
    creds = VenueCredentials(credentials_source="prompt_result", token="tok")
    await adapter.login(creds)
    assert adapter._token == "tok"


async def test_login_prompt_result_no_token():
    """prompt_result + token=None / "" は pydantic レイヤで ValidationError。

    Fix #6: VenueCredentials が prompt_result の token 必須を model validator で弾く。
    adapter.login まで届かない。
    """
    with pytest.raises(Exception):  # pydantic ValidationError
        VenueCredentials(credentials_source="prompt_result", token=None)
    with pytest.raises(Exception):
        VenueCredentials(credentials_source="prompt_result", token="")


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B1: login(env) wire-up — POST /token + token 保存
# kabu skill: env key は API password 1 個のみ (DEV_KABU_API_PASSWORD)
# prod 解禁は KABU_ALLOW_PROD=1 (kabusapi_url.base_url 経由で発火)
# ---------------------------------------------------------------------------


async def test_login_env_missing_api_password_raises(monkeypatch):
    monkeypatch.delenv("DEV_KABU_API_PASSWORD", raising=False)
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError, match="DEV_KABU_API_PASSWORD"):
        await KabuStationAdapter().login(creds)


async def test_login_env_does_not_leak_password_in_exception(monkeypatch):
    """R10 — exception message must not contain the credential value itself."""
    monkeypatch.delenv("DEV_KABU_API_PASSWORD", raising=False)
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError) as exc_info:
        await KabuStationAdapter().login(creds)
    msg = str(exc_info.value)
    assert "DEV_KABU_API_PASSWORD" in msg
    assert "credentials_source='env'" in msg


async def test_login_env_verify_success_stores_token(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "verify-pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "verify-token-xxxx"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))

    assert adapter._token == "verify-token-xxxx"


async def test_login_env_verify_posts_api_password(
    monkeypatch, httpx_mock: HTTPXMock
):
    """fetch_token は POST /token を 1 回叩き、APIPassword を JSON body に載せる。"""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "verify-pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    await KabuStationAdapter(environment="verify").login(
        VenueCredentials(credentials_source="env")
    )

    import json as _json
    requests = httpx_mock.get_requests()
    token_reqs = [r for r in requests if str(r.url).endswith("/token")]
    assert len(token_reqs) == 1
    body = _json.loads(token_reqs[0].content)
    assert body == {"APIPassword": "verify-pw"}


async def test_login_env_prod_without_allow_prod_raises(monkeypatch):
    """Production double-guard: KABU_ALLOW_PROD must be '1' (kabusapi_url 経由)."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "prod-pw")
    monkeypatch.delenv("KABU_ALLOW_PROD", raising=False)

    adapter = KabuStationAdapter(environment="prod")
    with pytest.raises(RuntimeError, match="KABU_ALLOW_PROD"):
        await adapter.login(VenueCredentials(credentials_source="env"))


async def test_login_env_prod_with_allow_prod_hits_prod_url(
    monkeypatch, httpx_mock: HTTPXMock
):
    """prod env + KABU_ALLOW_PROD=1 → 本番 18080 に POST、token 保存。"""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "prod-pw")
    monkeypatch.setenv("KABU_ALLOW_PROD", "1")
    httpx_mock.add_response(
        url=endpoint("token", env="prod"),
        method="POST",
        json={"ResultCode": 0, "Token": "prod-token-yyyy"},
    )

    adapter = KabuStationAdapter(environment="prod")
    await adapter.login(VenueCredentials(credentials_source="env"))

    assert adapter._token == "prod-token-yyyy"
    requests = httpx_mock.get_requests()
    assert any(":18080/" in str(r.url) for r in requests)


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B2: fetch_instruments MVP — 空 list 返却
# 理由 (handoff §「ユーザー決定事項」L84): kabu fetch_instruments は空 list。
# subscribe 時の /symbol lazy fetch は B4 以降。
# ---------------------------------------------------------------------------


async def test_fetch_instruments_returns_empty_list():
    """MVP: HTTP を叩かず空 list を返す (handoff ユーザー決定事項)。"""
    adapter = KabuStationAdapter()
    result = await adapter.fetch_instruments()
    assert result == []


async def test_fetch_instruments_returns_list_type():
    """戻り値は list (None や tuple ではない) — Protocol 適合のため。"""
    adapter = KabuStationAdapter()
    result = await adapter.fetch_instruments()
    assert isinstance(result, list)


async def test_fetch_instruments_does_not_require_login():
    """MVP: login 前でも呼べる (将来 lazy fetch 化したら login 必須に変える)。"""
    adapter = KabuStationAdapter()
    assert adapter._token is None
    result = await adapter.fetch_instruments()
    assert result == []


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B4-4a: _put_register helper
# Contract: PUT {base}/register, header X-API-KEY=<token>,
#           body {"Symbols": [{"Symbol": "<sym>", "Exchange": <int>}, ...]}
#           ResultCode 0 → True, それ以外 → False (logger.warning は kabusapi_ws 側)
# ---------------------------------------------------------------------------

async def test_put_register_posts_symbols_with_token_header(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn-xyz"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": [{"Symbol": "7203", "Exchange": 1}]},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    ok = await adapter._put_register([("7203", 1)])

    assert ok is True
    import json as _json
    put_reqs = [r for r in httpx_mock.get_requests() if r.method == "PUT"]
    assert len(put_reqs) == 1
    assert put_reqs[0].headers.get("X-API-KEY") == "tkn-xyz"
    body = _json.loads(put_reqs[0].content)
    assert body == {"Symbols": [{"Symbol": "7203", "Exchange": 1}]}


async def test_put_register_returns_false_on_nonzero_result_code(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Post-merge review HIGH-1: 4002001 -> KabuRegisterFullError (not silent False)."""
    from engine.exchanges.kabusapi_auth import KabuRegisterFullError

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 4002001, "Message": "register full"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    with pytest.raises(KabuRegisterFullError):
        await adapter._put_register([("7203", 1)])


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B4-4b: subscribe / unsubscribe 配線
# Contract:
#   - instrument_id "<sym>.TSE" を split → (sym, 1) で RegisterSet.register
#     → _put_register(all_symbols()) → _processors[sym] = KabuPushFrameProcessor
#   - "<sym>.OSE" 等 TSE 以外 suffix は ValueError (MVP: TSE=1 固定)
#   - login 前の subscribe は RuntimeError
#   - unsubscribe: RegisterSet.unregister → _put_register(残存銘柄で再送)
# ---------------------------------------------------------------------------


async def test_subscribe_calls_put_register_and_creates_processor(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": [{"Symbol": "7203", "Exchange": 1}]},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    import json as _json
    put_reqs = [r for r in httpx_mock.get_requests() if r.method == "PUT"]
    assert len(put_reqs) == 1
    body = _json.loads(put_reqs[0].content)
    assert body == {"Symbols": [{"Symbol": "7203", "Exchange": 1}]}
    assert ("7203", 1) in adapter._processors
    assert ("7203", 1) in adapter._register_set


async def test_subscribe_then_unsubscribe_replays_remaining_symbols(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    # 3 回分の PUT /register response を仕込む (subscribe x2 + unsubscribe x1)
    for _ in range(3):
        httpx_mock.add_response(
            url=endpoint("register", env="verify"),
            method="PUT",
            json={"ResultCode": 0, "RegistList": []},
        )

    adapter = KabuStationAdapter(environment="verify")
    # register は no-burst (capacity=1) のため 2 回目以降に rate-limit sleep が入る。
    # ユニットテストでは実時間 sleep させず、かつ event loop に yield して ws task が
    # 余分な register を発火しないよう、即時 (非 yield) の no-op sleep を inject する。
    async def _instant(_d):  # noqa: ANN001
        return
    adapter._rate_limit_sleep = _instant
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    await adapter.subscribe("9984.TSE", {"trades"})
    await adapter.unsubscribe("7203.TSE")

    import json as _json
    put_reqs = [r for r in httpx_mock.get_requests() if r.method == "PUT"]
    assert len(put_reqs) == 3
    last_body = _json.loads(put_reqs[-1].content)
    assert last_body == {"Symbols": [{"Symbol": "9984", "Exchange": 1}]}
    assert ("7203", 1) not in adapter._processors
    assert ("9984", 1) in adapter._processors


async def test_subscribe_rejects_non_tse_suffix(monkeypatch, httpx_mock: HTTPXMock):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    with pytest.raises(ValueError):
        await adapter.subscribe("7203.OSE", {"trades"})


async def test_subscribe_without_login_raises_runtime_error():
    adapter = KabuStationAdapter(environment="verify")
    with pytest.raises(RuntimeError, match="login"):
        await adapter.subscribe("7203.TSE", {"trades"})


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B4-4c: events() + _on_frame 配線 + logout cleanup
# ---------------------------------------------------------------------------


async def test_on_frame_emits_depth_and_trade_into_queue():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    adapter = KabuStationAdapter(environment="verify")
    adapter._token = "tkn"
    adapter._processors[("7203", 1)] = KabuPushFrameProcessor(symbol="7203")
    adapter._processors[("7203", 1)].process(
        {
            "CurrentPrice": 1000.0,
            "TradingVolume": 100.0,
            "CurrentPriceTime": "2026-05-18T09:00:00",
            "Sell1": {"Price": 1001.0, "Qty": 10.0},
            "Buy1": {"Price": 999.0, "Qty": 20.0},
        }
    )
    await adapter._on_frame(
        {
            "Symbol": "7203",
            "CurrentPrice": 1001.0,
            "TradingVolume": 150.0,
            "CurrentPriceTime": "2026-05-18T09:00:01",
            "Sell1": {"Price": 1002.0, "Qty": 5.0},
            "Buy1": {"Price": 1000.0, "Qty": 8.0},
        }
    )

    first = adapter._queue.get_nowait()
    second = adapter._queue.get_nowait()
    events = [first, second]
    depths = [e for e in events if isinstance(e, DepthUpdate)]
    trades = [e for e in events if isinstance(e, TradesUpdate)]
    assert len(depths) == 1
    assert len(trades) == 1
    assert depths[0].instrument_id == "7203.TSE"
    assert trades[0].instrument_id == "7203.TSE"
    assert trades[0].aggressor_side == "buy"
    assert trades[0].price == 1001.0
    assert trades[0].size == 50.0


async def test_on_frame_skips_trade_when_codec_returns_none():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    adapter = KabuStationAdapter(environment="verify")
    adapter._token = "tkn"
    adapter._processors[("7203", 1)] = KabuPushFrameProcessor(symbol="7203")

    await adapter._on_frame(
        {
            "Symbol": "7203",
            "CurrentPrice": 1000.0,
            "TradingVolume": 100.0,
            "CurrentPriceTime": "2026-05-18T09:00:00",
            "Sell1": {"Price": 1001.0, "Qty": 10.0},
            "Buy1": {"Price": 999.0, "Qty": 20.0},
        }
    )

    assert adapter._queue.qsize() == 1
    only = adapter._queue.get_nowait()
    assert isinstance(only, DepthUpdate)


async def test_events_yields_from_queue():
    adapter = KabuStationAdapter(environment="verify")
    adapter._queue.put_nowait(
        DepthUpdate(
            kind="depth",
            instrument_id="7203.TSE",
            ts_ns=0,
            bids=(),
            asks=(),
        )
    )

    agen = adapter.events()
    event = await asyncio.wait_for(agen.__anext__(), timeout=1.0)
    assert isinstance(event, DepthUpdate)
    assert event.instrument_id == "7203.TSE"


async def test_logout_cancels_ws_task_and_clears_state(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": []},
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    monkeypatch.setattr(
        "engine.exchanges.kabusapi.kabusapi_ws_connect", _fake_connect, raising=False
    )
    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    assert adapter._ws_task is not None
    assert not adapter._ws_task.done()

    await adapter.logout()

    assert adapter._token is None
    assert adapter._ws_task.cancelled() or adapter._ws_task.done()
    assert adapter._processors == {}
    assert len(adapter._register_set) == 0


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B4-4d: review fix — subscribe/login/unsubscribe robustness
# 1. subscribe: PUT /register が失敗したら state を rollback して raise
# 2. subscribe: 前回の WS task が done() なら再 spawn
# 3. login: logout 後の再 login で closed client を作り直す
# 4. unsubscribe: TSE 以外の suffix は ValueError (state は変更しない)
# ---------------------------------------------------------------------------


async def test_subscribe_rolls_back_state_on_register_failure(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 4002001, "Message": "register full"},
    )

    from engine.exchanges.kabusapi_auth import KabuRegisterFullError

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    with pytest.raises(KabuRegisterFullError):
        await adapter.subscribe("7203.TSE", {"trades"})

    assert adapter._processors == {}
    assert len(adapter._register_set) == 0
    assert adapter._ws_task is None


async def test_subscribe_respawns_ws_task_when_previous_done(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    for _ in range(2):
        httpx_mock.add_response(
            url=endpoint("register", env="verify"),
            method="PUT",
            json={"ResultCode": 0, "RegistList": []},
        )

    async def _fake_connect_immediate(**kwargs):
        return

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect_immediate)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    first_task = adapter._ws_task
    assert first_task is not None
    await asyncio.sleep(0)
    await asyncio.sleep(0)
    assert first_task.done()

    await adapter.subscribe("9984.TSE", {"trades"})
    second_task = adapter._ws_task
    assert second_task is not None
    assert second_task is not first_task


async def test_login_recreates_closed_client(monkeypatch, httpx_mock: HTTPXMock):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    for _ in range(2):
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 0, "Token": "tkn"},
        )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": []},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.logout()
    assert adapter._client.is_closed

    await adapter.login(VenueCredentials(credentials_source="env"))
    assert not adapter._client.is_closed
    ok = await adapter._put_register([("7203", 1)])
    assert ok is True


async def test_unsubscribe_rejects_non_tse_suffix(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": []},
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    with pytest.raises(ValueError, match="suffix|OSE"):
        await adapter.unsubscribe("7203.OSE")

    assert ("7203", 1) in adapter._processors
    assert ("7203", 1) in adapter._register_set

    await adapter.logout()


# ---------------------------------------------------------------------------
# Phase 8 §3.2 B4-4e: review fix —
#   High:   WS task silent death を events() に伝播
#   Medium①: 既存購読 symbol の再 subscribe で PUT 失敗時に既存 state を壊さない
#   Medium②: unsubscribe は PUT 成功後に local state を commit (server/local skew 防止)
# ---------------------------------------------------------------------------


async def test_events_raises_when_ws_task_dies(
    monkeypatch, httpx_mock: HTTPXMock
):
    """WS task が KabuConnectionError で死んだら events() は無音にならず raise する。"""
    from engine.exchanges.kabusapi_auth import KabuConnectionError

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": []},
    )

    async def _dying_connect(**kwargs):
        raise KabuConnectionError("ws upstream gone")

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _dying_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    # WS task が死んだら events() は永久 _queue.get() で待たず、例外を raise する
    events_iter = adapter.events()
    with pytest.raises(KabuConnectionError, match="ws upstream gone"):
        await asyncio.wait_for(events_iter.__anext__(), timeout=1.0)


async def test_subscribe_duplicate_with_put_failure_preserves_existing_state(
    monkeypatch, httpx_mock: HTTPXMock
):
    """既に subscribe 済み symbol の再 subscribe で PUT が失敗しても、
    既存の register / processor を壊さない (Medium ①)。"""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    # 1 回目 subscribe: PUT 成功
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": []},
    )
    # 2 回目 subscribe (同 symbol): PUT 失敗
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 4002001, "Message": "register full"},
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    from engine.exchanges.kabusapi_auth import KabuRegisterFullError

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    first_processor = adapter._processors[("7203", 1)]

    with pytest.raises(KabuRegisterFullError):
        await adapter.subscribe("7203.TSE", {"trades"})

    # 既存 state は破壊されていない
    assert ("7203", 1) in adapter._register_set
    assert adapter._processors.get(("7203", 1)) is first_processor

    await adapter.logout()


async def test_unsubscribe_raises_and_preserves_state_on_put_failure(
    monkeypatch, httpx_mock: HTTPXMock
):
    """unsubscribe の PUT が失敗したら local state を消さずに raise する (Medium ②)。"""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    # subscribe: PUT 成功
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 0, "RegistList": []},
    )
    # unsubscribe: PUT 失敗
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 4002001, "Message": "register sync failed"},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    from engine.exchanges.kabusapi_auth import KabuRegisterFullError

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    with pytest.raises(KabuRegisterFullError):
        await adapter.unsubscribe("7203.TSE")

    # PUT が失敗したら local state はそのまま (server/local skew 防止)
    assert ("7203", 1) in adapter._register_set
    assert ("7203", 1) in adapter._processors

    # logout will attempt PUT /unregister/all (post-merge MEDIUM-2) — provide mock
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )
    await adapter.logout()


# ===========================================================================
# Post-merge review fixes (2026-05-20)
# ===========================================================================


async def test_put_register_http_401_raises_api_error(
    monkeypatch, httpx_mock: HTTPXMock
):
    """HIGH-1: HTTP 401 from PUT /register must surface as KabuApiError
    (via check_response, not silent False)."""
    from engine.exchanges.kabusapi_auth import KabuApiError

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        status_code=401,
        json={"Code": 4001005, "Message": "token expired"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    with pytest.raises(KabuApiError):
        await adapter._put_register([("7203", 1)])


async def test_put_register_code_4001005_raises_token_expired(
    monkeypatch, httpx_mock: HTTPXMock
):
    """HIGH-1: HTTP 200 + Code 4001005 → KabuTokenExpiredError (re-auth path)."""
    from engine.exchanges.kabusapi_auth import KabuTokenExpiredError

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 4001005, "Message": "token expired"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    with pytest.raises(KabuTokenExpiredError):
        await adapter._put_register([("7203", 1)])


async def test_on_frame_keys_processor_by_symbol_and_exchange():
    """HIGH-2: same Symbol under different Exchange must use distinct processors.

    R4: kabu symbol key = "<Symbol>@<Exchange>". Keying by Symbol alone
    collides TSE(1) and 名証(3) state.
    """
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    adapter = KabuStationAdapter(environment="verify")
    adapter._token = "tkn"
    proc_tse = KabuPushFrameProcessor(symbol="5401")
    proc_meisho = KabuPushFrameProcessor(symbol="5401")
    adapter._processors[("5401", 1)] = proc_tse
    adapter._processors[("5401", 3)] = proc_meisho

    await adapter._on_frame(
        {
            "Symbol": "5401",
            "Exchange": 1,
            "CurrentPrice": 100.0,
            "TradingVolume": 10.0,
            "CurrentPriceTime": "2026-05-20T09:00:00",
        }
    )
    await adapter._on_frame(
        {
            "Symbol": "5401",
            "Exchange": 3,
            "CurrentPrice": 200.0,
            "TradingVolume": 20.0,
            "CurrentPriceTime": "2026-05-20T09:00:00",
        }
    )

    assert proc_tse._prev_volume == 10.0
    assert proc_meisho._prev_volume == 20.0


async def test_ws_reconnect_resets_processors(monkeypatch, httpx_mock: HTTPXMock):
    """HIGH-3: on WS reconnect, every processor's reset() must be called
    BEFORE new frames are dispatched (codec docstring contract)."""
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
        is_reusable=True,
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )

    reset_count = {"n": 0}

    class _SpyProcessor(KabuPushFrameProcessor):
        def reset(self):
            reset_count["n"] += 1
            super().reset()

    import engine.exchanges.kabusapi_ws as _ws_mod

    async def _fake_connect(*, env, on_message, register_set, put_register,
                            on_reconnect=None):
        # Simulate a reconnect by invoking the on_reconnect hook once.
        if on_reconnect is not None:
            result = on_reconnect()
            if asyncio.iscoroutine(result):
                await result

    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    spy = _SpyProcessor(symbol="7203")
    adapter._processors[("7203", 1)] = spy
    adapter._register_set.register("7203", 1)

    await adapter.subscribe("7203.TSE", {"trades"})

    for _ in range(10):
        await asyncio.sleep(0)

    assert reset_count["n"] >= 1, "on_reconnect callback should reset processors"

    await adapter.logout()


async def test_put_register_rate_limited_to_10_per_second(
    monkeypatch, httpx_mock: HTTPXMock
):
    """MEDIUM-1: PUT /register is info-class (R5: 10 req/s). >10 calls in
    <1s must observe delays."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
        is_reusable=True,
    )

    fake_now = {"t": 0.0}
    sleeps: list[float] = []

    def _time_source() -> float:
        return fake_now["t"]

    async def _fake_sleep(d: float) -> None:
        sleeps.append(d)
        fake_now["t"] += d

    adapter = KabuStationAdapter(environment="verify", time_source=_time_source)
    await adapter.login(VenueCredentials(credentials_source="env"))
    # Inject the sleep stub into the adapter (not global asyncio.sleep, since
    # httpx mock transport may need real sleep).
    adapter._rate_limit_sleep = _fake_sleep  # type: ignore[attr-defined]

    for _ in range(11):
        await adapter._put_register([("7203", 1)])

    assert any(s > 0 for s in sleeps), (
        f"expected rate-limiter to insert a sleep; got sleeps={sleeps}"
    )


async def test_logout_issues_unregister_all(monkeypatch, httpx_mock: HTTPXMock):
    """MEDIUM-2: logout() must call PUT /unregister/all (R6 cleanup) when
    there is at least one prior registration."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    await adapter.logout()

    put_reqs = [
        r for r in httpx_mock.get_requests()
        if r.method == "PUT" and str(r.url).endswith("/unregister/all")
    ]
    assert len(put_reqs) == 1


async def test_logout_tolerates_unregister_all_failure(
    monkeypatch, httpx_mock: HTTPXMock
):
    """MEDIUM-2: logout() must tolerate unregister/all failures (token may
    already be invalid)."""
    import httpx as _httpx

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
    )

    def _boom(request):
        raise _httpx.ConnectError("body gone")

    httpx_mock.add_callback(
        _boom, url=endpoint("unregister/all", env="verify"), method="PUT"
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    await adapter.logout()
    assert adapter._token is None


async def test_last_error_exposes_ws_task_exception(
    monkeypatch, httpx_mock: HTTPXMock
):
    """MEDIUM-3: WS task death must surface via adapter.last_error side-channel."""
    from engine.exchanges.kabusapi_auth import KabuConnectionError

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
        is_reusable=True,
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )

    async def _dying_connect(**kwargs):
        raise KabuConnectionError("ws upstream gone (last_error test)")

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _dying_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    for _ in range(20):
        await asyncio.sleep(0)
        if adapter.last_error is not None:
            break

    assert isinstance(adapter.last_error, KabuConnectionError)
    assert "ws upstream gone" in str(adapter.last_error)

    # Round2: consume the unregister/all matcher via logout (also covers
    # pytest-httpx teardown ERROR caveat in MEMORY.md).
    await adapter.logout()


# ===========================================================================
# Round 2 post-merge review fixes (2026-05-20)
# ===========================================================================


async def test_logout_propagates_cancelled_error_during_unregister_all(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Round2 HIGH-1: cancelling logout() while PUT /unregister/all is in
    flight must propagate asyncio.CancelledError (not swallow it via
    `except BaseException`)."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    # Replace _client.put with one that hangs forever — we want to cancel
    # while it is awaiting.
    hang_event = asyncio.Event()

    async def _hanging_put(*args, **kwargs):
        await hang_event.wait()  # never set
        raise AssertionError("unreachable")

    adapter._client.put = _hanging_put  # type: ignore[assignment]

    task = asyncio.create_task(adapter.logout())
    # Let logout() reach the put() await.
    for _ in range(5):
        await asyncio.sleep(0)

    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task


async def test_logout_unregister_all_has_timeout(monkeypatch, httpx_mock: HTTPXMock):
    """Round2 HIGH-2: PUT /unregister/all must have its own inner timeout so
    a hung kabu body cannot block logout indefinitely. logout() should
    complete within a small window even if the call hangs."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    # Simulate a kabu body that hangs indefinitely. Without an inner
    # timeout on the PUT call, logout() would block forever — wait_for at
    # 2.0s would raise. With the fix the inner httpx timeout (a few sec)
    # surfaces, the except clause logs and continues, and logout completes.
    async def _hanging_put(*args, **kwargs):
        # Honour any timeout= passed by the adapter. If the adapter passes
        # no timeout, this sleeps long enough to exceed the outer wait_for.
        timeout = kwargs.get("timeout")
        if timeout is not None and hasattr(timeout, "read") and timeout.read:
            await asyncio.sleep(min(timeout.read, 0.5))
            import httpx as _httpx
            raise _httpx.ReadTimeout("kabu hung (inner timeout)", request=None)
        await asyncio.sleep(30.0)  # bury the outer wait_for if no inner timeout

    adapter._client.put = _hanging_put  # type: ignore[assignment]

    # If the inner timeout is missing, _hanging_put sleeps 30s and the outer
    # wait_for raises TimeoutError. With the fix logout() returns within 2s.
    await asyncio.wait_for(adapter.logout(), timeout=2.0)
    assert adapter._token is None


async def test_login_clears_prior_last_error(monkeypatch, httpx_mock: HTTPXMock):
    """Round2 MEDIUM-1: a successful login() must clear adapter._last_error
    so callers polling `last_error` for fresh sessions don't see stale state."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    adapter = KabuStationAdapter(environment="verify")
    adapter._last_error = RuntimeError("prior session boom")  # type: ignore[assignment]

    await adapter.login(VenueCredentials(credentials_source="env"))

    assert adapter.last_error is None


async def test_on_frame_missing_exchange_resolves_via_processors(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Round2 MEDIUM-2a: when Exchange is missing from a frame and only one
    processor matches the symbol, route to that processor (do NOT default
    to TSE=1)."""
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))

    # Only register for 名証 (Exchange=3) — NOT TSE.
    seen_calls: list[dict] = []

    class _SpyProc(KabuPushFrameProcessor):
        def process(self, msg):
            seen_calls.append(msg)
            return None, None

    adapter._processors[("5401", 3)] = _SpyProc(symbol="5401")

    # Frame omits Exchange. Old code would default to 1 and silently drop;
    # new code must look up the only matching symbol → (5401, 3).
    await adapter._on_frame(
        {
            "Symbol": "5401",
            "CurrentPrice": 100.0,
            "TradingVolume": 10.0,
            "CurrentPriceTime": "2026-05-20T09:00:00",
        }
    )

    assert len(seen_calls) == 1, (
        f"frame should route to the single registered (5401,3) processor; "
        f"got {len(seen_calls)} calls"
    )


async def test_on_frame_missing_exchange_drops_when_ambiguous(
    monkeypatch, httpx_mock: HTTPXMock, caplog
):
    """Round2 MEDIUM-2b: when Exchange is missing AND multiple processors
    match the symbol, the frame must be dropped (no processor invoked)
    and a warning logged."""
    import logging as _logging

    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))

    invoked: list[tuple[str, int]] = []

    class _SpyProc(KabuPushFrameProcessor):
        def __init__(self, symbol, exchange):
            super().__init__(symbol=symbol)
            self._exchange = exchange

        def process(self, msg):
            invoked.append((self.symbol, self._exchange))
            return None, None

    adapter._processors[("5401", 1)] = _SpyProc("5401", 1)
    adapter._processors[("5401", 3)] = _SpyProc("5401", 3)

    with caplog.at_level(_logging.WARNING, logger="engine.exchanges.kabusapi"):
        await adapter._on_frame(
            {
                "Symbol": "5401",
                "CurrentPrice": 100.0,
                "TradingVolume": 10.0,
                "CurrentPriceTime": "2026-05-20T09:00:00",
            }
        )

    assert invoked == [], (
        f"ambiguous frame must be dropped; got invocations on {invoked}"
    )
    assert any(
        "ambiguous" in rec.message.lower() or "5401" in rec.message
        for rec in caplog.records
    ), "expected a warning log about ambiguous symbol routing"


async def test_ws_on_reconnect_not_invoked_on_first_connect(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Round2 MEDIUM-3: kabusapi_ws.connect() must NOT invoke on_reconnect
    on the first successful connect (only on 2nd+). This guards against
    regressions where a reset would wipe brand-new processor state before
    the first frame arrives."""
    import engine.exchanges.kabusapi_ws as _ws_mod

    reconnect_calls = {"n": 0}

    class _FakeWs:
        def __init__(self):
            self._sent: list[str] = []

        async def __aenter__(self):
            return self

        async def __aexit__(self, *_a):
            return False

        async def recv(self):
            # End the connect() loop by raising CancelledError, which
            # propagates out of `connect()`.
            raise asyncio.CancelledError()

        async def send(self, msg):
            self._sent.append(msg)

    def _fake_connect_factory(url, **kwargs):
        return _FakeWs()

    monkeypatch.setattr(_ws_mod.websockets, "connect", _fake_connect_factory)

    async def _on_reconnect():
        reconnect_calls["n"] += 1

    async def _put_register(symbols):
        return True

    from engine.exchanges.kabusapi_register import RegisterSet
    rs = RegisterSet()

    async def _on_message(_msg):
        pass

    with pytest.raises(asyncio.CancelledError):
        await _ws_mod.connect(
            env="verify",
            on_message=_on_message,
            register_set=rs,
            put_register=_put_register,
            on_reconnect=_on_reconnect,
        )

    assert reconnect_calls["n"] == 0, (
        f"on_reconnect must NOT fire on first connect; got {reconnect_calls['n']} calls"
    )


async def test_ws_reconnect_resets_processors_gated_by_call_count(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Round2 MEDIUM-3: the adapter-level reconnect-reset test must mirror the
    real ws gating (fire on_reconnect only on the 2nd+ connect). A fake that
    fires on first connect would let a buggy adapter regression through."""
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
        is_reusable=True,
    )
    httpx_mock.add_response(
        url=endpoint("unregister/all", env="verify"),
        method="PUT",
        json={"Code": 0},
        is_reusable=True,
    )

    reset_count = {"n": 0}

    class _SpyProcessor(KabuPushFrameProcessor):
        def reset(self):
            reset_count["n"] += 1
            super().reset()

    import engine.exchanges.kabusapi_ws as _ws_mod

    call_count = {"n": 0}

    async def _fake_connect(*, env, on_message, register_set, put_register,
                            on_reconnect=None):
        # Mirror real gating: fire on_reconnect ONLY on the 2nd+ call.
        call_count["n"] += 1
        if call_count["n"] >= 2 and on_reconnect is not None:
            result = on_reconnect()
            if asyncio.iscoroutine(result):
                await result
        # Simulate the per-connect lifetime by returning quickly; the adapter
        # will see the task finish but that's fine for the assertion.

    # We need 2 calls; have _fake_connect simulate a reconnect by being
    # invoked twice from a wrapping loop.
    async def _looping_connect(**kwargs):
        await _fake_connect(**kwargs)
        await _fake_connect(**kwargs)

    monkeypatch.setattr(_ws_mod, "connect", _looping_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    spy = _SpyProcessor(symbol="7203")
    adapter._processors[("7203", 1)] = spy
    adapter._register_set.register("7203", 1)

    await adapter.subscribe("7203.TSE", {"trades"})

    for _ in range(10):
        await asyncio.sleep(0)

    assert reset_count["n"] == 1, (
        f"reset() must fire exactly once (on the 2nd connect), got "
        f"{reset_count['n']}"
    )

    await adapter.logout()

    await adapter.logout()


# ===========================================================================
# Round 3 post-merge review fixes (2026-05-20)
# ===========================================================================


async def test_logout_tolerates_oserror_during_unregister_all(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Round3 MEDIUM-1: a closed/half-open client can raise raw OSError /
    ConnectionResetError from the transport during shutdown races. These
    must be swallowed by the best-effort cleanup, not propagated."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"Code": 0, "RegistList": []},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    async def _oserror_put(*args, **kwargs):
        raise OSError("connection reset")

    adapter._client.put = _oserror_put  # type: ignore[assignment]

    # Must not propagate OSError out of best-effort logout.
    await adapter.logout()
    assert adapter._token is None


async def test_on_frame_ambiguous_exchange_warns_once_per_symbol(
    monkeypatch, httpx_mock: HTTPXMock, caplog
):
    """Round3 MEDIUM-2: ambiguous-Exchange warning must be rate-limited to
    once per symbol per session. Subsequent drops log at DEBUG to avoid
    log spam at kabu PUSH rates."""
    import logging as _logging

    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))

    class _SpyProc(KabuPushFrameProcessor):
        def process(self, msg):
            return None, None

    adapter._processors[("5401", 1)] = _SpyProc(symbol="5401")
    adapter._processors[("5401", 3)] = _SpyProc(symbol="5401")

    frame = {
        "Symbol": "5401",
        "CurrentPrice": 100.0,
        "TradingVolume": 10.0,
        "CurrentPriceTime": "2026-05-20T09:00:00",
    }

    with caplog.at_level(_logging.DEBUG, logger="engine.exchanges.kabusapi"):
        for _ in range(10):
            await adapter._on_frame(dict(frame))

    warnings = [
        r for r in caplog.records
        if r.levelno == _logging.WARNING and "5401" in r.message
    ]
    assert len(warnings) == 1, (
        f"expected exactly 1 WARNING for ambiguous symbol 5401, got {len(warnings)}"
    )


async def test_on_frame_ambiguous_warning_resets_on_login(
    monkeypatch, httpx_mock: HTTPXMock, caplog
):
    """Round3 MEDIUM-2: the warned-once set must reset on login() so a fresh
    session emits the warning again."""
    import logging as _logging

    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
        is_reusable=True,
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))

    class _SpyProc(KabuPushFrameProcessor):
        def process(self, msg):
            return None, None

    adapter._processors[("5401", 1)] = _SpyProc(symbol="5401")
    adapter._processors[("5401", 3)] = _SpyProc(symbol="5401")

    frame = {
        "Symbol": "5401",
        "CurrentPrice": 100.0,
        "TradingVolume": 10.0,
        "CurrentPriceTime": "2026-05-20T09:00:00",
    }

    with caplog.at_level(_logging.WARNING, logger="engine.exchanges.kabusapi"):
        await adapter._on_frame(dict(frame))
        await adapter._on_frame(dict(frame))  # second time: DEBUG (no WARNING)

    initial_warnings = [
        r for r in caplog.records
        if r.levelno == _logging.WARNING and "5401" in r.message
    ]
    assert len(initial_warnings) == 1

    # Fresh login → reset warned set.
    await adapter.login(VenueCredentials(credentials_source="env"))
    adapter._processors[("5401", 1)] = _SpyProc(symbol="5401")
    adapter._processors[("5401", 3)] = _SpyProc(symbol="5401")

    caplog.clear()
    with caplog.at_level(_logging.WARNING, logger="engine.exchanges.kabusapi"):
        await adapter._on_frame(dict(frame))

    post_login_warnings = [
        r for r in caplog.records
        if r.levelno == _logging.WARNING and "5401" in r.message
    ]
    assert len(post_login_warnings) == 1, (
        "warned set must reset on login() so a fresh session emits WARNING again"
    )


async def test_login_does_not_clear_last_error_on_prompt_result_empty_token():
    """Round3 MEDIUM-3: when login() raises early (prompt_result with empty
    token), _last_error must retain its prior value. Clearing only happens
    on the success path."""
    adapter = KabuStationAdapter()
    prior = ValueError("prior")
    adapter._last_error = prior

    # Bypass pydantic by constructing a creds object with empty token.
    # The adapter's own early raise (PROMPT_RESULT_MISSING_TOKEN) must fire
    # without first clearing _last_error.
    class _Creds:
        credentials_source = "prompt_result"
        token = ""

    with pytest.raises(ValueError, match="PROMPT_RESULT_MISSING_TOKEN"):
        await adapter.login(_Creds())  # type: ignore[arg-type]

    assert adapter.last_error is prior, (
        "login() must not clear _last_error before the early-validation raise"
    )


async def test_login_clears_last_error_on_success_only(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Round3 MEDIUM-3: success path still clears _last_error (preserve the
    Round 2 contract)."""
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )

    adapter = KabuStationAdapter(environment="verify")
    adapter._last_error = RuntimeError("prior")

    await adapter.login(VenueCredentials(credentials_source="env"))

    assert adapter.last_error is None
