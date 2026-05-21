"""Phase 10 Step 2 — LiveStrategyStateMachine の単体テスト (§1.2)。

    IDLE → LOADING → READY → RUNNING → (PAUSED) → STOPPING → STOPPED
                                  ↘ ERROR (safety rail violation / venue error)
"""

from __future__ import annotations

import pytest

from engine.live.strategy_state_machine import (
    ERROR,
    IDLE,
    LOADING,
    PAUSED,
    READY,
    RUNNING,
    STOPPED,
    STOPPING,
    InvalidLiveStrategyTransition,
    LiveStrategyStateMachine,
)


def test_initial_state_is_idle():
    sm = LiveStrategyStateMachine()
    assert sm.current == IDLE
    assert sm.error_code is None
    assert not sm.is_terminal


def test_happy_path_full_lifecycle():
    sm = LiveStrategyStateMachine()
    for target in (LOADING, READY, RUNNING, PAUSED, RUNNING, STOPPING, STOPPED):
        sm.transition_to(target)
    assert sm.current == STOPPED
    assert sm.is_terminal


def test_illegal_transition_raises():
    sm = LiveStrategyStateMachine()
    with pytest.raises(InvalidLiveStrategyTransition, match="IDLE -> RUNNING"):
        sm.transition_to(RUNNING)


def test_unknown_state_raises():
    sm = LiveStrategyStateMachine()
    with pytest.raises(InvalidLiveStrategyTransition, match="unknown"):
        sm.transition_to("WAT")


def test_error_sets_code_and_state_from_running():
    sm = LiveStrategyStateMachine()
    for target in (LOADING, READY, RUNNING):
        sm.transition_to(target)
    sm.error("MAX_DAILY_LOSS_EXCEEDED")
    assert sm.current == ERROR
    assert sm.error_code == "MAX_DAILY_LOSS_EXCEEDED"
    # ERROR → STOPPED (内部 StopLiveStrategy → run を STOPPED に, §1.3)
    sm.transition_to(STOPPED)
    assert sm.is_terminal


def test_error_reachable_from_loading():
    sm = LiveStrategyStateMachine()
    sm.transition_to(LOADING)
    sm.error("STRATEGY_LOAD_FAILED")
    assert sm.current == ERROR


def test_stopped_is_terminal():
    sm = LiveStrategyStateMachine()
    for target in (LOADING, READY, RUNNING, STOPPING, STOPPED):
        sm.transition_to(target)
    with pytest.raises(InvalidLiveStrategyTransition):
        sm.transition_to(RUNNING)


def test_error_from_terminal_is_rejected():
    sm = LiveStrategyStateMachine()
    for target in (LOADING, READY, RUNNING, STOPPING, STOPPED):
        sm.transition_to(target)
    with pytest.raises(InvalidLiveStrategyTransition):
        sm.error("LATE")


def test_is_active_and_is_running_helpers():
    sm = LiveStrategyStateMachine()
    assert not sm.is_active
    for target in (LOADING, READY, RUNNING):
        sm.transition_to(target)
    assert sm.is_active and sm.is_running
    sm.transition_to(PAUSED)
    assert sm.is_active and not sm.is_running  # paused は新規発注ゲートで deny (§1.2)


def test_ready_can_stop_before_running():
    sm = LiveStrategyStateMachine()
    sm.transition_to(LOADING)
    sm.transition_to(READY)
    sm.transition_to(STOPPING)  # READY からの中止を許可
    sm.transition_to(STOPPED)
    assert sm.is_terminal
