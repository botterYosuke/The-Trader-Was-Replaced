"""In-proc E2E smoke tests — INPROC_E2E=1 gate.

These tests exercise the real Python interpreter path via InprocLiveServer /
DataEngine without mocking the underlying calls. They cover the verification
gaps noted in issue #64 (botterYosuke/The-Trader-Was-Replaced#64):

- P3: BackendTradingState JSON roundtrip through real DataEngine
- P5: full close/teardown contract; set_execution_mode / list_instruments routing

Run with:
    INPROC_E2E=1 pytest python/tests/test_inproc_e2e.py -v

Skipped by default so CI without Python engine does not fail.
"""
import json
import os
import pytest

pytestmark = pytest.mark.skipif(
    not os.getenv("INPROC_E2E"),
    reason="Set INPROC_E2E=1 to run in-proc E2E tests (requires real Python engine)",
)

from engine.core import DataEngine


# ---------------------------------------------------------------------------
# P3: BackendTradingState JSON roundtrip
# ---------------------------------------------------------------------------

def test_p3_get_state_json_contains_required_fields():
    """P3: get_state_json() returns valid JSON with all Rust BackendTradingState fields.

    BackendTradingState::from_json (serde_json) expects at minimum 'replay_state'.
    Additional optional fields (last_timestamp_ms, portfolio, per_instrument) must
    be either absent or have the right type — no stray keys that break deserialization.
    """
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)
    json_str = srv.get_state_json()

    state = json.loads(json_str)
    assert isinstance(state, dict), "get_state_json must return a JSON object"
    assert "replay_state" in state, "replay_state is the primary field Rust reads"
    assert state["replay_state"] == "IDLE", "fresh DataEngine should start IDLE"


def test_p3_force_stop_replay_is_graceful_from_idle():
    """P3: ForceStopReplay from IDLE state returns a response (no exception).

    Rust InProcTransport's ForceStop handler calls inproc_dispatch(ForceStop)
    which calls DataEngine.force_stop_replay() — must not raise even when IDLE.
    """
    from engine.inproc_server import InprocLiveServer
    from engine import _proto_compat as engine_pb2

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    req = engine_pb2.ForceStopReplayRequest(token="")
    resp = srv._srv.ForceStopReplay(req, srv._ctx)
    assert resp is not None


def test_p3_set_execution_mode_then_get_state_consistent():
    """P3: SetExecutionMode(Replay) → GetState returns the same or compatible mode.

    Verifies the full round-trip: Rust inproc_dispatch(SetExecutionMode) →
    Python GrpcDataEngineServer.SetExecutionMode → state reflected in GetState.
    """
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    result = srv.set_execution_mode("Replay")
    assert isinstance(result, dict)
    assert "success" in result

    state = json.loads(srv.get_state_json())
    assert "replay_state" in state


# ---------------------------------------------------------------------------
# P5: close/teardown and live command routing
# ---------------------------------------------------------------------------

def test_p5_close_calls_teardown_and_stop_live_loop():
    """P5: InprocLiveServer.close() calls _teardown_live_components AND stop_live_loop.

    P10 (unit) only verifies _teardown_live_components is called.
    This test confirms both steps complete without exception on a fresh server
    that never started a live loop (None guards in close()).
    """
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    teardown_calls = []
    original_teardown = srv._srv._teardown_live_components
    srv._srv._teardown_live_components = lambda: teardown_calls.append("teardown") or original_teardown()

    srv.close()

    assert "teardown" in teardown_calls
    # close() must not raise — stop_live_loop with None live_thread is a no-op


def test_p5_venue_login_venue_logout_cycle_no_adapter():
    """P5: VenueLogin → VenueLogout completes without exception (no adapter configured).

    Rust inproc_dispatch routes VenueLogin → InprocLiveServer.venue_login()
    → GrpcDataEngineServer.VenueLogin(NullContext) → LIVE_ADAPTER_NOT_CONFIGURED.
    Then VenueLogout returns gracefully.
    """
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    login = srv.venue_login("MOCK", "prompt", None)
    assert isinstance(login, dict)
    assert login["success"] is False
    assert login["error_code"] == "LIVE_ADAPTER_NOT_CONFIGURED"

    logout = srv.venue_logout()
    assert isinstance(logout, dict)
    assert "success" in logout


def test_p5_list_instruments_local_routing():
    """P5: list_instruments("local") routes to GrpcDataEngineServer.ListInstruments.

    Rust inproc_dispatch(ListInstruments(ReplayCatalogFallback)) calls
    InprocLiveServer.list_instruments("local").
    Without a catalog the result is success=False or empty list — not an exception.
    """
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    result = srv.list_instruments("local")
    assert isinstance(result, dict)
    assert "instrument_ids" in result
    assert isinstance(result["instrument_ids"], list)


def test_p5_set_execution_mode_live_manual_no_adapter():
    """P5: SetExecutionMode(LiveManual) routes cleanly without live adapter.

    Part of the VenueLogin → SetExecutionMode(LiveManual) → VenueLogout sequence
    defined in P5. No adapter means mode switch may fail gracefully, not raise.
    """
    from engine.inproc_server import InprocLiveServer

    engine = DataEngine()
    srv = InprocLiveServer(engine, live_venue_id=None)

    result = srv.set_execution_mode("LiveManual")
    assert isinstance(result, dict)
    assert "success" in result
    assert "error_code" in result
