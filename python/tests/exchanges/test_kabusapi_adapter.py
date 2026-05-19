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
    monkeypatch.setenv("DEV_KABU_API_PASSWORD", "pw")
    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "tkn"},
    )
    httpx_mock.add_response(
        url=endpoint("register", env="verify"),
        method="PUT",
        json={"ResultCode": 4002001, "Message": "register full"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    ok = await adapter._put_register([("7203", 1)])
    assert ok is False


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
    assert "7203" in adapter._processors
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
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    await adapter.subscribe("9984.TSE", {"trades"})
    await adapter.unsubscribe("7203.TSE")

    import json as _json
    put_reqs = [r for r in httpx_mock.get_requests() if r.method == "PUT"]
    assert len(put_reqs) == 3
    last_body = _json.loads(put_reqs[-1].content)
    assert last_body == {"Symbols": [{"Symbol": "9984", "Exchange": 1}]}
    assert "7203" not in adapter._processors
    assert "9984" in adapter._processors


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
    adapter._processors["7203"] = KabuPushFrameProcessor(symbol="7203")
    adapter._processors["7203"].process(
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
    adapter._processors["7203"] = KabuPushFrameProcessor(symbol="7203")

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
        json={"ResultCode": 4002001, "Message": "register full"},
    )

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    with pytest.raises(RuntimeError, match="register"):
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

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    with pytest.raises(ValueError, match="suffix|OSE"):
        await adapter.unsubscribe("7203.OSE")

    assert "7203" in adapter._processors
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
        json={"ResultCode": 4002001, "Message": "register full"},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})
    first_processor = adapter._processors["7203"]

    with pytest.raises(RuntimeError, match="register"):
        await adapter.subscribe("7203.TSE", {"trades"})

    # 既存 state は破壊されていない
    assert ("7203", 1) in adapter._register_set
    assert adapter._processors.get("7203") is first_processor

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
        json={"ResultCode": 4002001, "Message": "register sync failed"},
    )

    async def _fake_connect(**kwargs):
        await asyncio.Event().wait()

    import engine.exchanges.kabusapi_ws as _ws_mod
    monkeypatch.setattr(_ws_mod, "connect", _fake_connect)

    adapter = KabuStationAdapter(environment="verify")
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades"})

    with pytest.raises(RuntimeError, match="unregister"):
        await adapter.unsubscribe("7203.TSE")

    # PUT が失敗したら local state はそのまま (server/local skew 防止)
    assert ("7203", 1) in adapter._register_set
    assert "7203" in adapter._processors

    await adapter.logout()
