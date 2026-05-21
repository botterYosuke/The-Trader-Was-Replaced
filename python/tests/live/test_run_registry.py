"""Phase 10 Step 2 — RunRegistry の単体テスト (§2.6 / §0.7 / M4 / M6)。"""

from __future__ import annotations

import pytest

from engine.live.run_registry import (
    DuplicateStrategyInstrument,
    LiveStrategyAlreadyRunning,
    RunRegistry,
)
from engine.live.strategy_state_machine import LiveStrategyStateMachine, RUNNING, READY, LOADING, STOPPING, STOPPED


def _advance(sm: LiveStrategyStateMachine, *states):
    for s in states:
        sm.transition_to(s)
    return sm


def _running_sm() -> LiveStrategyStateMachine:
    return _advance(LiveStrategyStateMachine(), LOADING, READY, RUNNING)


def _register(reg: RunRegistry, run_id="R1", strategy_id="S1", instrument_id="1301.TSE",
              nautilus_strategy_id="LIVE-R1", venue="TACHIBANA", sm=None):
    return reg.register(
        run_id=run_id,
        strategy_id=strategy_id,
        instrument_id=instrument_id,
        nautilus_strategy_id=nautilus_strategy_id,
        venue=venue,
        started_ts_ms=1_700_000_000_000,
        state_machine=sm or _running_sm(),
    )


def test_register_and_get():
    reg = RunRegistry()
    rec = _register(reg)
    assert reg.get("R1") is rec
    assert rec.strategy_id == "S1"
    assert rec.instrument_id == "1301.TSE"
    assert rec.nautilus_strategy_id == "LIVE-R1"
    assert rec.venue == "TACHIBANA"


def test_get_unknown_returns_none():
    assert RunRegistry().get("nope") is None


def test_second_active_run_rejected_by_single_run_limit():
    reg = RunRegistry()  # default max_active_live_auto_runs = 1
    _register(reg, run_id="R1")
    with pytest.raises(LiveStrategyAlreadyRunning):
        _register(reg, run_id="R2", strategy_id="S2", nautilus_strategy_id="LIVE-R2")


def test_can_register_again_after_unregister():
    reg = RunRegistry()
    _register(reg, run_id="R1")
    reg.unregister("R1")
    rec2 = _register(reg, run_id="R2", strategy_id="S2", nautilus_strategy_id="LIVE-R2")
    assert reg.get("R2") is rec2
    assert reg.get("R1") is None


def test_terminal_run_does_not_occupy_slot():
    reg = RunRegistry()
    sm1 = _advance(LiveStrategyStateMachine(), LOADING, READY, RUNNING, STOPPING, STOPPED)
    _register(reg, run_id="R1", sm=sm1)
    # R1 は STOPPED（terminal）なのでスロットを占有しない → R2 を登録できる
    rec2 = _register(reg, run_id="R2", strategy_id="S2", nautilus_strategy_id="LIVE-R2")
    assert reg.get("R2") is rec2


def test_duplicate_strategy_instrument_rejected():
    reg = RunRegistry(max_active_live_auto_runs=2)  # Phase 11 想定の複数許可
    _register(reg, run_id="R1", strategy_id="S1", instrument_id="1301.TSE")
    with pytest.raises(DuplicateStrategyInstrument):
        _register(reg, run_id="R2", strategy_id="S1", instrument_id="1301.TSE",
                  nautilus_strategy_id="LIVE-R2")


def test_same_strategy_different_instrument_allowed_when_max_raised():
    reg = RunRegistry(max_active_live_auto_runs=2)
    _register(reg, run_id="R1", strategy_id="S1", instrument_id="1301.TSE")
    rec2 = _register(reg, run_id="R2", strategy_id="S1", instrument_id="7203.TSE",
                     nautilus_strategy_id="LIVE-R2")
    assert reg.get("R2") is rec2


def test_run_id_for_nautilus_strategy():
    reg = RunRegistry()
    _register(reg, run_id="R1", nautilus_strategy_id="LIVE-R1")
    assert reg.run_id_for_nautilus_strategy("LIVE-R1") == "R1"
    assert reg.run_id_for_nautilus_strategy("MANUAL-001") is None


def test_list_active_excludes_terminal():
    reg = RunRegistry()
    sm = _running_sm()
    _register(reg, run_id="R1", sm=sm)
    assert [r.run_id for r in reg.list_active()] == ["R1"]
    sm.transition_to(STOPPING)
    sm.transition_to(STOPPED)
    assert reg.list_active() == []


def test_unregister_unknown_is_noop():
    reg = RunRegistry()
    assert reg.unregister("nope") is False
    _register(reg, run_id="R1")
    assert reg.unregister("R1") is True


def test_terminal_runs_evicted_beyond_history_cap_oldest_first():
    """STOPPED run は post-stop 照会用に残すが、上限超過分は古い順に退避する。"""
    reg = RunRegistry(max_terminal_history=2)
    for i in range(4):
        sm = _advance(LiveStrategyStateMachine(), LOADING, READY, RUNNING, STOPPING, STOPPED)
        reg.register(
            run_id=f"R{i}",
            strategy_id="S1",
            instrument_id="1301.TSE",
            nautilus_strategy_id=f"LIVE-R{i}",
            venue="TACHIBANA",
            started_ts_ms=1_700_000_000_000 + i,  # 昇順 = R0 が最古
            state_machine=sm,
        )
    # 終端 run は新しい 2 件 (R2, R3) だけ残り、古い R0/R1 は退避される。
    assert reg.get("R0") is None
    assert reg.get("R1") is None
    assert reg.get("R2") is not None
    assert reg.get("R3") is not None
