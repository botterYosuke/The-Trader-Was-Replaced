"""Phase 9 Step 7: server_grpc の Venue Health Watchdog 配線 (§3.5)。

- `_publish_venue_logout(venue)` が VenueLogoutDetected を BackendEventBus に push する。
- watchdog 起動は `hasattr(adapter, "check_health")` で gate される (kabu のみ・mock は無し)。
"""
from __future__ import annotations

from engine.core import DataEngine
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.server_grpc import GrpcDataEngineServer


def _servicer() -> GrpcDataEngineServer:
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)
    return GrpcDataEngineServer("tkn", engine, mode_manager=mm, venue_sm=venue_sm)


def test_publish_venue_logout_pushes_venue_logout_detected() -> None:
    servicer = _servicer()
    sub = servicer._backend_event_bus.subscribe()
    try:
        # publish は同期 (threadsafe queue) なので read 前に確実に enqueue される。
        servicer._publish_venue_logout("KABU")
        event = next(iter(sub))
    finally:
        sub.close()
    assert event.WhichOneof("payload") == "venue_logout_detected"
    assert event.venue_logout_detected.venue == "KABU"


def test_health_watchdog_gated_on_check_health_capability() -> None:
    """kabu adapter (check_health あり) は watchdog 対象、mock (なし) は対象外。
    server_grpc の `hasattr(adapter, "check_health")` gate と同じ判定。"""
    from engine.exchanges.kabusapi import KabuStationAdapter
    from engine.live.mock_adapter import MockVenueAdapter

    assert hasattr(KabuStationAdapter(environment="verify"), "check_health")
    assert not hasattr(MockVenueAdapter(), "check_health")
