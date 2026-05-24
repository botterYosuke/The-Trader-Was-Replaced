import logging

from engine.core import DataEngine
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.server_grpc import GrpcDataEngineServer


def _make_servicer():
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)
    return GrpcDataEngineServer(
        "test-token", engine, mode_manager=mm, venue_sm=venue_sm
    )


class _OkComponent:
    async def start(self):
        return None


def test_bg_component_start_logs_mode(caplog):
    """issue #29 Slice1 S1.5: bg component (account sync) を login 後に起動したとき、
    起動した事実と current execution mode が判別できる INFO ログを出す。
    実機で 'never called'（Manual で sync 未起動）を診断するための観測点。"""
    servicer = _make_servicer()
    servicer.mode_manager.current_mode = "LiveManual"

    with caplog.at_level(logging.INFO):
        servicer._start_bg_component_after_login(_OkComponent(), "account sync")

    msgs = " | ".join(r.getMessage() for r in caplog.records)
    assert "account sync" in msgs
    assert "LiveManual" in msgs
    assert "start" in msgs.lower()  # "started" 等で起動した事実が分かる
