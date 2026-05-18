"""Tests for TachibanaAdapter skeleton (Phase 8 §1.3)."""

import asyncio

import pytest

from engine.exchanges.tachibana import TachibanaAdapter
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


def test_login_raises_not_implemented():
    creds = VenueCredentials(credentials_source="prompt")
    with pytest.raises(NotImplementedError):
        asyncio.run(TachibanaAdapter().login(creds))


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
    a._session = {"token": "x"}
    asyncio.run(a.logout())
    assert a._session is None
