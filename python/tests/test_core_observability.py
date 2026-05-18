from engine.core import DataEngine
from engine.live.state_machine import VenueStateMachine


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
