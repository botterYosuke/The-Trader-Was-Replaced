from engine.core import DataEngine
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager


def test_data_engine_accepts_none_state_machine():
    """DataEngine は state_machine=None でも初期化できる（既存呼び出し互換）"""
    eng = DataEngine(state_machine=None)
    assert eng.state_machine is None


def test_data_engine_holds_injected_state_machine():
    """注入された VenueStateMachine が保持され、遷移が反映される"""
    sm = VenueStateMachine()
    eng = DataEngine(state_machine=sm)

    assert eng.state_machine is sm
    assert eng.state_machine.current == "DISCONNECTED"

    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")
    assert eng.state_machine.current == "CONNECTED"


def test_get_current_state_venue_state_is_disconnected_when_no_state_machine():
    """state_machine=None のとき、venue_state は明示的に "DISCONNECTED" を返す（None ではなく文字列。UI 判定容易化のため）"""
    eng = DataEngine(state_machine=None)

    state = eng.get_current_state()

    assert state.venue_state == "DISCONNECTED"


def test_get_current_state_venue_state_reflects_state_machine_current():
    """state_machine 注入時、venue_state は state_machine.current を反映する"""
    sm = VenueStateMachine()
    eng = DataEngine(state_machine=sm)

    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")

    state = eng.get_current_state()

    assert state.venue_state == "CONNECTED"


def test_get_current_state_execution_mode_default_is_replay_when_no_mode_manager():
    """mode_manager 未注入時、execution_mode は TradingState default の "Replay" を返す"""
    eng = DataEngine(state_machine=None)

    state = eng.get_current_state()

    assert state.execution_mode == "Replay"


def test_get_current_state_execution_mode_reflects_mode_manager():
    """ModeManager 注入時、execution_mode は ModeManager.current_mode を反映する"""
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")

    eng = DataEngine(state_machine=sm)
    mm = ModeManager(venue_sm=sm, replay_engine=eng)
    eng.attach_mode_manager(mm)

    mm.set_execution_mode("LiveManual")

    state = eng.get_current_state()

    assert state.execution_mode == "LiveManual"


def test_get_current_state_venue_id_default_is_none():
    """venue_id 未注入時、TradingState.venue_id は None"""
    eng = DataEngine(state_machine=None)

    state = eng.get_current_state()

    assert state.venue_id is None


def test_get_current_state_venue_id_reflects_injected_value():
    """DataEngine(venue_id='TACHIBANA') 注入時、state.venue_id == 'TACHIBANA'"""
    eng = DataEngine(state_machine=None, venue_id="TACHIBANA")

    state = eng.get_current_state()

    assert state.venue_id == "TACHIBANA"


def test_get_current_state_subscribed_instruments_default_empty():
    """subscribe API 未呼び出し時、subscribed_instruments は空 list"""
    eng = DataEngine(state_machine=None)

    state = eng.get_current_state()

    assert state.subscribed_instruments == []


def test_get_current_state_subscribed_instruments_reflects_internal_list():
    """DataEngine._subscribed_instruments への書き込みが state に反映される（Phase 8 後半で公開 API 化予定）"""
    eng = DataEngine(state_machine=None)
    eng._subscribed_instruments = ["1301.TSE", "5401.TSE"]

    state = eng.get_current_state()

    assert state.subscribed_instruments == ["1301.TSE", "5401.TSE"]


def test_get_current_state_instruments_loaded_default_is_zero():
    """ListInstruments 未実行時、instruments_loaded は 0"""
    eng = DataEngine(state_machine=None)

    state = eng.get_current_state()

    assert state.instruments_loaded == 0


def test_get_current_state_instruments_loaded_reflects_subscribed_count():
    """_subscribed_instruments の件数が instruments_loaded に反映される
    (Rust 側 BackendStatusUpdate::VenueChanged.instruments_loaded: u32 への配線元)"""
    eng = DataEngine(state_machine=None)
    eng._subscribed_instruments = ["1301.TSE", "5401.TSE", "7203.TSE"]

    state = eng.get_current_state()

    assert state.instruments_loaded == 3
