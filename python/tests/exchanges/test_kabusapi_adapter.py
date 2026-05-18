"""Tests for KabuStationAdapter skeleton (Phase 8 §1.3 / §3.2 B1)."""

import asyncio

import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.kabusapi import KabuStationAdapter
from engine.exchanges.kabusapi_url import endpoint
from engine.live.adapter import LiveVenueAdapter, VenueCredentials


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


def test_events_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        KabuStationAdapter().events()


def test_logout_clears_token():
    a = KabuStationAdapter()
    a._token = "abc"
    asyncio.run(a.logout())
    assert a._token is None


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
