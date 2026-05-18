"""RED tests for build_live_adapter_factory (Phase 8 §3.2 C1.1)."""

from __future__ import annotations

import pytest

from engine.live.adapter import LiveVenueAdapter
from engine.live.live_adapter_factory import (
    build_live_adapter_factory,
    UnknownVenueError,
)
from engine.exchanges.tachibana import TachibanaAdapter
from engine.exchanges.kabusapi import KabuStationAdapter


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
