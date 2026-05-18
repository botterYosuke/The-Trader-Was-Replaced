import pytest
from engine.mode_manager import ModeManager
from engine.live.state_machine import VenueStateMachine


class _FakeReplayEngine:
    def __init__(self, state: str = "IDLE") -> None:
        self.replay_state = state


# --- 初期状態 ---

def test_initial_execution_mode_is_replay():
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine())
    assert sm.current_mode == "Replay"


# --- LiveManual ガード ---

def test_set_live_manual_rejected_when_venue_disconnected():
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine())
    with pytest.raises(ValueError, match="EXECUTION_MODE_PRECONDITION"):
        sm.set_execution_mode("LiveManual")
    assert sm.current_mode == "Replay"  # 状態は変わらない


def test_set_live_manual_succeeds_when_venue_connected():
    venue = VenueStateMachine()
    venue.transition_to("AUTHENTICATING")
    venue.transition_to("CONNECTED")
    sm = ModeManager(venue, _FakeReplayEngine())
    result = sm.set_execution_mode("LiveManual")
    assert result == "LiveManual"
    assert sm.current_mode == "LiveManual"


def test_set_live_manual_succeeds_when_venue_subscribed():
    """CONNECTED 以上 (SUBSCRIBED 含む) なら通る"""
    venue = VenueStateMachine()
    venue.transition_to("AUTHENTICATING")
    venue.transition_to("CONNECTED")
    venue.transition_to("SUBSCRIBED")
    sm = ModeManager(venue, _FakeReplayEngine())
    assert sm.set_execution_mode("LiveManual") == "LiveManual"


# --- LiveAuto ガード (Phase 10 用スタブだが allow) ---

def test_set_live_auto_succeeds_when_venue_connected():
    venue = VenueStateMachine()
    venue.transition_to("AUTHENTICATING")
    venue.transition_to("CONNECTED")
    sm = ModeManager(venue, _FakeReplayEngine())
    assert sm.set_execution_mode("LiveAuto") == "LiveAuto"


def test_set_live_auto_rejected_when_venue_disconnected():
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine())
    with pytest.raises(ValueError, match="EXECUTION_MODE_PRECONDITION"):
        sm.set_execution_mode("LiveAuto")


# --- Replay ガード ---

def test_set_replay_rejected_when_replay_state_idle():
    venue = VenueStateMachine()
    venue.transition_to("AUTHENTICATING")
    venue.transition_to("CONNECTED")
    sm = ModeManager(venue, _FakeReplayEngine(state="IDLE"))
    # まず LiveManual に進めておく (初期 Replay からの切替を意味あるテストにする)
    sm.set_execution_mode("LiveManual")
    with pytest.raises(ValueError, match="EXECUTION_MODE_PRECONDITION"):
        sm.set_execution_mode("Replay")
    assert sm.current_mode == "LiveManual"


def test_set_replay_succeeds_when_replay_state_loaded():
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine(state="LOADED"))
    assert sm.set_execution_mode("Replay") == "Replay"


def test_set_replay_succeeds_when_replay_state_running():
    """LOADED 以上 (RUNNING/PAUSED 含む) なら通る"""
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine(state="RUNNING"))
    assert sm.set_execution_mode("Replay") == "Replay"


# --- 並列稼働シナリオ (ADR: 認証と Execution は排他しない) ---

def test_can_switch_back_to_replay_after_live_when_both_ready():
    """LiveManual モード中に Replay engine が LOADED に進んだら Replay 切替が可能"""
    venue = VenueStateMachine()
    venue.transition_to("AUTHENTICATING")
    venue.transition_to("CONNECTED")
    replay = _FakeReplayEngine(state="IDLE")
    sm = ModeManager(venue, replay)
    sm.set_execution_mode("LiveManual")
    # 後から replay が LOADED に
    replay.replay_state = "LOADED"
    assert sm.set_execution_mode("Replay") == "Replay"


# --- 不明な mode ---

def test_unknown_mode_rejected():
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine(state="LOADED"))
    with pytest.raises(ValueError, match="EXECUTION_MODE_PRECONDITION"):
        sm.set_execution_mode("Foo")


# --- ModeManager は LoadReplayData / VenueLogin 操作をガードしない ---

def test_mode_manager_does_not_intercept_venue_or_replay_operations():
    """
    ADR: 認証と Execution は排他しない。
    ModeManager は set_execution_mode() でしかガードしない。
    venue_sm / replay_engine への直接操作は ModeManager に通知不要で素通しになる。
    (= ModeManager に login() や load_replay() のような proxy メソッドが存在しないことを
     表明するテスト。getattr で確認する。)
    """
    sm = ModeManager(VenueStateMachine(), _FakeReplayEngine())
    assert not hasattr(sm, "login")
    assert not hasattr(sm, "logout")
    assert not hasattr(sm, "load_replay")
    assert not hasattr(sm, "start_engine")
