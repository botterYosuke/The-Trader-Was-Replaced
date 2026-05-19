"""Tests for build_live_adapter_factory (Phase 8 §3.2 C1.1 + Step 6 env_hint)."""

from __future__ import annotations

import pytest

from engine.live.adapter import LiveVenueAdapter
from engine.live.live_adapter_factory import (
    build_live_adapter_factory,
    UnknownVenueError,
    _resolve_tachibana_env,
    _resolve_kabu_env,
)
from engine.exchanges.tachibana import TachibanaAdapter
from engine.exchanges.kabusapi import KabuStationAdapter


# --- Legacy tests (backward compat: factory() called with no args) ----------

def test_build_live_adapter_factory_tachibana_returns_tachibana_adapter():
    factory = build_live_adapter_factory("TACHIBANA")
    adapter = factory()
    assert isinstance(adapter, TachibanaAdapter)
    assert adapter.venue_id == "TACHIBANA"


def test_build_live_adapter_factory_kabu_returns_kabusapi_adapter():
    factory = build_live_adapter_factory("KABU")
    adapter = factory()
    assert isinstance(adapter, KabuStationAdapter)
    assert adapter.venue_id == "KABU"


def test_build_live_adapter_factory_unknown_venue_raises():
    with pytest.raises(UnknownVenueError):
        build_live_adapter_factory("BINANCE")


def test_build_live_adapter_factory_returns_callable_protocol_conformant():
    """factory() の戻り値は LiveVenueAdapter Protocol に準拠する。"""
    factory = build_live_adapter_factory("TACHIBANA")
    adapter = factory()
    # Protocol は runtime-checkable でないかもしれないので duck check
    assert hasattr(adapter, "venue_id")
    assert hasattr(adapter, "login")
    assert hasattr(adapter, "fetch_instruments")
    assert hasattr(adapter, "subscribe")
    assert hasattr(adapter, "events")


# --- Step 6: env_hint tests -------------------------------------------------

def test_factory_tachibana_demo():
    factory = build_live_adapter_factory("TACHIBANA")
    adapter = factory("demo")
    assert isinstance(adapter, TachibanaAdapter)
    assert adapter._env == "demo"


def test_factory_tachibana_prod():
    factory = build_live_adapter_factory("TACHIBANA")
    adapter = factory("prod")
    assert adapter._env == "prod"


def test_factory_kabu_verify():
    factory = build_live_adapter_factory("KABU")
    adapter = factory("verify")
    assert isinstance(adapter, KabuStationAdapter)
    assert adapter._env == "verify"


def test_factory_kabu_prod():
    factory = build_live_adapter_factory("KABU")
    adapter = factory("prod")
    assert adapter._env == "prod"


def test_factory_default_env_when_hint_none():
    tachi_factory = build_live_adapter_factory("TACHIBANA")
    assert tachi_factory(None)._env == "demo"

    kabu_factory = build_live_adapter_factory("KABU")
    assert kabu_factory(None)._env == "verify"


def test_factory_invalid_hint_raises():
    tachi_factory = build_live_adapter_factory("TACHIBANA")
    with pytest.raises(ValueError, match="invalid Tachibana"):
        tachi_factory("verify")

    kabu_factory = build_live_adapter_factory("KABU")
    with pytest.raises(ValueError, match="invalid kabu"):
        kabu_factory("demo")


def test_factory_unknown_venue_raises():
    with pytest.raises(UnknownVenueError):
        build_live_adapter_factory("UNKNOWN")


# --- _resolve_* unit tests --------------------------------------------------

def test_resolve_tachibana_env_defaults_to_demo():
    assert _resolve_tachibana_env(None) == "demo"
    assert _resolve_tachibana_env("") == "demo"


def test_resolve_tachibana_env_valid():
    assert _resolve_tachibana_env("demo") == "demo"
    assert _resolve_tachibana_env("prod") == "prod"


def test_resolve_tachibana_env_invalid_raises():
    with pytest.raises(ValueError):
        _resolve_tachibana_env("verify")
    with pytest.raises(ValueError):
        _resolve_tachibana_env("staging")


def test_resolve_kabu_env_defaults_to_verify():
    assert _resolve_kabu_env(None) == "verify"
    assert _resolve_kabu_env("") == "verify"


def test_resolve_kabu_env_valid():
    assert _resolve_kabu_env("verify") == "verify"
    assert _resolve_kabu_env("prod") == "prod"


def test_resolve_kabu_env_invalid_raises():
    with pytest.raises(ValueError):
        _resolve_kabu_env("demo")
    with pytest.raises(ValueError):
        _resolve_kabu_env("staging")
