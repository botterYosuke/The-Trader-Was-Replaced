"""Live venue adapter factory (Phase 8 §3.2 C1.2).

venue 名から LiveVenueAdapter を遅延生成する factory を返す。
副作用 (instantiate) は factory() 呼び出し時まで遅延される。
"""

from __future__ import annotations

from typing import Callable, Optional

from engine.exchanges.kabusapi import KabuStationAdapter
from engine.exchanges.tachibana import TachibanaAdapter
from engine.live.adapter import LiveVenueAdapter


class UnknownVenueError(Exception):
    """未知の venue 名が指定されたとき。"""


def build_live_adapter_factory(venue: str) -> Callable[[Optional[str]], LiveVenueAdapter]:
    """venue 名から LiveVenueAdapter factory (closure) を返す。

    venue 検証は本関数呼び出し時に行い、未知 venue は即 UnknownVenueError を raise する。
    adapter の instantiate は返却 closure が呼ばれたタイミングまで遅延される。

    Args:
        venue: "TACHIBANA" / "KABU" / "MOCK"

    Returns:
        Callable[[Optional[str]], LiveVenueAdapter] —
        env_hint=None でデフォルト環境 (TACHIBANA=demo, KABU=verify) を使用する。
    """
    if venue == "TACHIBANA":
        return lambda env_hint=None: TachibanaAdapter(environment=_resolve_tachibana_env(env_hint))
    if venue == "KABU":
        return lambda env_hint=None: KabuStationAdapter(environment=_resolve_kabu_env(env_hint))
    # D26: MOCK venue for development/testing without real venue connection
    if venue == "MOCK":
        from engine.live.mock_adapter import MockVenueAdapter
        return lambda env_hint=None: MockVenueAdapter()
    raise UnknownVenueError(f"unknown venue: {venue!r}")


def _resolve_env(hint: Optional[str], default: str, valid: tuple, venue_name: str) -> str:
    if not hint:
        return default
    if hint in valid:
        return hint
    raise ValueError(f"invalid {venue_name} environment_hint: {hint!r}")


def _resolve_tachibana_env(hint: Optional[str]) -> str:
    return _resolve_env(hint, "demo", ("demo", "prod"), "Tachibana")


def _resolve_kabu_env(hint: Optional[str]) -> str:
    return _resolve_env(hint, "verify", ("verify", "prod"), "kabu")
