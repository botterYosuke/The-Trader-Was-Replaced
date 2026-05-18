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


def test_fetch_instruments_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        asyncio.run(KabuStationAdapter().fetch_instruments())


def test_subscribe_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        asyncio.run(KabuStationAdapter().subscribe("5401.TSE", {"price"}))


def test_unsubscribe_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        asyncio.run(KabuStationAdapter().unsubscribe("5401.TSE"))


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
