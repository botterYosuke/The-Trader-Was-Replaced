"""Tests for TachibanaAdapter (Phase 8 §1.3 skeleton + §3.2 A1.5 login wire-up)."""

import asyncio
import json
import re

import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.tachibana import TachibanaAdapter
from engine.exchanges.tachibana_auth import TachibanaSession
from engine.exchanges.tachibana_url import BASE_URL_DEMO
from engine.live.adapter import LiveVenueAdapter, VenueCredentials


def test_venue_id_is_tachibana():
    assert TachibanaAdapter().venue_id == "TACHIBANA"


def test_protocol_compliance():
    assert isinstance(TachibanaAdapter(), LiveVenueAdapter)


def test_default_environment_is_demo():
    assert TachibanaAdapter()._env == "demo"


def test_environment_demo_accepted():
    assert TachibanaAdapter(environment="demo")._env == "demo"


def test_environment_prod_accepted():
    assert TachibanaAdapter(environment="prod")._env == "prod"


def test_invalid_environment_raises():
    with pytest.raises(ValueError):
        TachibanaAdapter(environment="staging")  # type: ignore[arg-type]


def test_fetch_instruments_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        asyncio.run(TachibanaAdapter().fetch_instruments())


def test_subscribe_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        asyncio.run(TachibanaAdapter().subscribe("7203.TSE", {"price"}))


def test_unsubscribe_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        asyncio.run(TachibanaAdapter().unsubscribe("7203.TSE"))


def test_events_raises_not_implemented():
    with pytest.raises(NotImplementedError):
        TachibanaAdapter().events()


def test_logout_clears_session():
    a = TachibanaAdapter()
    a._session = "sentinel"  # type: ignore[assignment]
    asyncio.run(a.logout())
    assert a._session is None


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A1.5: login() wire-up
# ---------------------------------------------------------------------------

_DEMO_BASE = BASE_URL_DEMO.value
_DEMO_HOST_PATH = _DEMO_BASE.removeprefix("https://").removesuffix("/")
_AUTH_URL_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}auth/\?")


def _ok_login_payload() -> dict:
    return {
        "p_no": "1",
        "p_sd_date": "2026.04.25-10:00:00.000",
        "p_errno": "0",
        "p_err": "",
        "sCLMID": "CLMAuthLoginAck",
        "sResultCode": "0",
        "sResultText": "",
        "sZyoutoekiKazeiC": "1",
        "sKinsyouhouMidokuFlg": "0",
        "sUrlRequest": f"{_DEMO_BASE}request/ND=/",
        "sUrlMaster": f"{_DEMO_BASE}master/ND=/",
        "sUrlPrice": f"{_DEMO_BASE}price/ND=/",
        "sUrlEvent": f"{_DEMO_BASE}event/ND=/",
        "sUrlEventWebSocket": f"wss://{_DEMO_HOST_PATH}/event_ws/ND=/",
    }


def _add_login_response(httpx_mock: HTTPXMock, payload: dict) -> None:
    httpx_mock.add_response(
        url=_AUTH_URL_RE,
        method="GET",
        content=json.dumps(payload, ensure_ascii=False).encode("shift_jis"),
    )


async def test_login_session_cache_raises_not_implemented():
    creds = VenueCredentials(credentials_source="session_cache")
    with pytest.raises(NotImplementedError):
        await TachibanaAdapter().login(creds)


async def test_login_prompt_raises_not_implemented():
    creds = VenueCredentials(credentials_source="prompt")
    with pytest.raises(NotImplementedError):
        await TachibanaAdapter().login(creds)


async def test_login_env_missing_user_id_raises(monkeypatch):
    monkeypatch.delenv("DEV_TACHIBANA_USER_ID", raising=False)
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError, match="DEV_TACHIBANA_USER_ID"):
        await TachibanaAdapter().login(creds)


async def test_login_env_missing_password_raises(monkeypatch):
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.delenv("DEV_TACHIBANA_PASSWORD", raising=False)
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError, match="DEV_TACHIBANA_PASSWORD"):
        await TachibanaAdapter().login(creds)


async def test_login_env_does_not_leak_credentials_in_exception(monkeypatch):
    """R10 — exception message must not contain the credential values."""
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "secret-uid-xyz")
    monkeypatch.delenv("DEV_TACHIBANA_PASSWORD", raising=False)
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError) as exc_info:
        await TachibanaAdapter().login(creds)
    assert "secret-uid-xyz" not in str(exc_info.value)


async def test_login_env_demo_success_stores_session(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    _add_login_response(httpx_mock, _ok_login_payload())

    adapter = TachibanaAdapter(environment="demo")
    await adapter.login(VenueCredentials(credentials_source="env"))

    assert isinstance(adapter._session, TachibanaSession)
    assert adapter._session.url_event_ws.startswith("wss://")


async def test_login_env_demo_uses_adapter_p_no_counter(
    monkeypatch, httpx_mock: HTTPXMock
):
    """R4 — the adapter's PNoCounter must advance across login calls."""
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    _add_login_response(httpx_mock, _ok_login_payload())
    _add_login_response(httpx_mock, _ok_login_payload())

    adapter = TachibanaAdapter(environment="demo")
    before = adapter._p_no_counter.peek()
    await adapter.login(VenueCredentials(credentials_source="env"))
    after_first = adapter._p_no_counter.peek()
    await adapter.login(VenueCredentials(credentials_source="env"))
    after_second = adapter._p_no_counter.peek()

    assert after_first == before + 1
    assert after_second == after_first + 1


async def test_login_env_prod_without_allow_prod_raises(monkeypatch):
    """Production double-guard: TACHIBANA_ALLOW_PROD must be '1'."""
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    monkeypatch.delenv("TACHIBANA_ALLOW_PROD", raising=False)

    adapter = TachibanaAdapter(environment="prod")
    with pytest.raises(RuntimeError, match="TACHIBANA_ALLOW_PROD"):
        await adapter.login(VenueCredentials(credentials_source="env"))
