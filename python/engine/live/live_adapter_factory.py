"""Live venue adapter factory (Phase 8 §3.2 C1.2).

venue 名から LiveVenueAdapter を遅延生成する factory を返す。
副作用 (instantiate) は factory() 呼び出し時まで遅延される。
"""

from __future__ import annotations

from typing import Callable

from engine.exchanges.kabusapi import KabuStationAdapter
from engine.exchanges.tachibana import TachibanaAdapter
from engine.live.adapter import LiveVenueAdapter


class UnknownVenueError(Exception):
    """未知の venue 名が指定されたとき。"""


def build_live_adapter_factory(venue: str) -> Callable[[], LiveVenueAdapter]:
    """venue 名から LiveVenueAdapter factory (closure) を返す。

    venue 検証は本関数呼び出し時に行い、未知 venue は即 UnknownVenueError を raise する。
    adapter の instantiate は返却 closure が呼ばれたタイミングまで遅延される。
    """
    if venue == "TACHIBANA":
        return lambda: TachibanaAdapter()
    if venue == "KABU":
        return lambda: KabuStationAdapter()
    # D26: MOCK venue for development/testing without real venue connection
    if venue == "MOCK":
        from engine.live.mock_adapter import MockVenueAdapter
        return lambda: MockVenueAdapter()
    raise UnknownVenueError(f"unknown venue: {venue!r}")
