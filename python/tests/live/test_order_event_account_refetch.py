"""Slice 4 (#29): EC stream order events trigger account_sync.force_resync().

OrderAccepted / PARTIALLY_FILLED / FILLED / CANCELED は BP・Positions に影響するため、
_publish_order_event がこれらのステータスを受けたとき account_sync.force_resync() を
live_loop にスケジュールすることを保証する。
REJECTED / SUBMITTED 等は口座残高に影響しないためスキップする。
"""
from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest

from engine.core import DataEngine
from engine.mode_manager import ModeManager
from engine.live.order_types import OrderEventData
from engine.live.state_machine import VenueStateMachine
from engine.server_grpc import GrpcDataEngineServer


def _make_servicer() -> GrpcDataEngineServer:
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)
    return GrpcDataEngineServer("token", engine, mode_manager=mm, venue_sm=venue_sm)


def _make_ev(status: str) -> OrderEventData:
    return OrderEventData(
        order_id="O1",
        venue_order_id="V1",
        client_order_id="C1",
        status=status,
        filled_qty=0.0,
        avg_price=0.0,
        ts_ms=0,
    )


@pytest.mark.parametrize("status", ["ACCEPTED", "PARTIALLY_FILLED", "FILLED", "CANCELED"])
def test_order_event_schedules_force_resync_for_state_changing_status(status: str) -> None:
    """口座残高に影響するステータスは force_resync() をスケジュールする。"""
    servicer = _make_servicer()
    mock_sync = MagicMock()
    servicer._account_sync = mock_sync
    mock_loop = MagicMock()
    mock_loop.is_running.return_value = True
    servicer._live_loop = mock_loop

    with patch.object(servicer, "publish_backend_event"), patch(
        "engine.server_grpc.asyncio.run_coroutine_threadsafe"
    ) as mock_rct:
        servicer._publish_order_event(_make_ev(status))

    mock_rct.assert_called_once()
    _, called_loop = mock_rct.call_args[0]
    assert called_loop is mock_loop


@pytest.mark.parametrize("status", ["SUBMITTED", "REJECTED", "INITIALIZED"])
def test_order_event_no_resync_for_non_state_changing_status(status: str) -> None:
    """口座残高に影響しないステータスは force_resync() をスケジュールしない。"""
    servicer = _make_servicer()
    mock_sync = MagicMock()
    servicer._account_sync = mock_sync
    mock_loop = MagicMock()
    mock_loop.is_running.return_value = True
    servicer._live_loop = mock_loop

    with patch.object(servicer, "publish_backend_event"), patch(
        "engine.server_grpc.asyncio.run_coroutine_threadsafe"
    ) as mock_rct:
        servicer._publish_order_event(_make_ev(status))

    mock_rct.assert_not_called()


def test_order_event_no_resync_when_account_sync_absent() -> None:
    """account_sync が None のとき（ログイン前）は force_resync() をスケジュールしない。"""
    servicer = _make_servicer()
    servicer._account_sync = None
    mock_loop = MagicMock()
    mock_loop.is_running.return_value = True
    servicer._live_loop = mock_loop

    with patch.object(servicer, "publish_backend_event"), patch(
        "engine.server_grpc.asyncio.run_coroutine_threadsafe"
    ) as mock_rct:
        servicer._publish_order_event(_make_ev("FILLED"))

    mock_rct.assert_not_called()


def test_order_event_no_resync_when_loop_not_running() -> None:
    """live_loop が停止中のとき force_resync() をスケジュールしない（teardown 後の EC イベント）。"""
    servicer = _make_servicer()
    mock_sync = MagicMock()
    servicer._account_sync = mock_sync
    mock_loop = MagicMock()
    mock_loop.is_running.return_value = False
    servicer._live_loop = mock_loop

    with patch.object(servicer, "publish_backend_event"), patch(
        "engine.server_grpc.asyncio.run_coroutine_threadsafe"
    ) as mock_rct:
        servicer._publish_order_event(_make_ev("FILLED"))

    mock_rct.assert_not_called()
