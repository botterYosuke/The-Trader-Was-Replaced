import pytest

from engine.live.state_machine import VenueStateMachine, InvalidVenueTransition


def test_initial_state_is_disconnected():
    sm = VenueStateMachine()
    assert sm.current == "DISCONNECTED"


def test_happy_path_disconnected_to_subscribed():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    assert sm.current == "AUTHENTICATING"
    sm.transition_to("CONNECTED")
    assert sm.current == "CONNECTED"
    sm.transition_to("SUBSCRIBED")
    assert sm.current == "SUBSCRIBED"


def test_subscribed_to_reconnecting_and_back_to_subscribed():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")
    sm.transition_to("SUBSCRIBED")
    sm.transition_to("RECONNECTING")
    assert sm.current == "RECONNECTING"
    sm.transition_to("SUBSCRIBED")
    assert sm.current == "SUBSCRIBED"


def test_reconnecting_to_error():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")
    sm.transition_to("SUBSCRIBED")
    sm.transition_to("RECONNECTING")
    sm.transition_to("ERROR")
    assert sm.current == "ERROR"


def test_error_resets_to_disconnected_via_reset():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")
    sm.transition_to("SUBSCRIBED")
    sm.transition_to("RECONNECTING")
    sm.transition_to("ERROR")
    sm.reset()
    assert sm.current == "DISCONNECTED"


def test_error_to_disconnected_via_transition_to_is_allowed():
    """Post-merge fix: ERROR → DISCONNECTED via transition_to is allowed
    so recovery paths (e.g. _fail()) can explicitly drop the venue without
    going through reset()."""
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("ERROR")
    sm.transition_to("DISCONNECTED")
    assert sm.current == "DISCONNECTED"


def test_invalid_transition_disconnected_to_connected_raises():
    sm = VenueStateMachine()
    with pytest.raises(InvalidVenueTransition):
        sm.transition_to("CONNECTED")


def test_invalid_transition_authenticating_to_subscribed_raises():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    with pytest.raises(InvalidVenueTransition):
        sm.transition_to("SUBSCRIBED")


def test_invalid_transition_connected_to_reconnecting_raises():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")
    with pytest.raises(InvalidVenueTransition):
        sm.transition_to("RECONNECTING")


def test_unknown_state_raises():
    sm = VenueStateMachine()
    with pytest.raises(InvalidVenueTransition):
        sm.transition_to("FOO")


def test_reset_from_non_error_is_allowed_idempotent():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("CONNECTED")
    sm.transition_to("SUBSCRIBED")
    sm.reset()
    assert sm.current == "DISCONNECTED"


def test_authenticating_to_error_on_auth_failure():
    sm = VenueStateMachine()
    sm.transition_to("AUTHENTICATING")
    sm.transition_to("ERROR")
    assert sm.current == "ERROR"
