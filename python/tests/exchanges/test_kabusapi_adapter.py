"""Tests for KabuStationAdapter skeleton (Phase 8 §1.3)."""

import asyncio

import pytest

from engine.exchanges.kabusapi import KabuStationAdapter
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


def test_login_env_raises_not_implemented():
    creds = VenueCredentials(credentials_source="env")
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
