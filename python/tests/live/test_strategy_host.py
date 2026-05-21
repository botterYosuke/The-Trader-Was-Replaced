"""Phase 10 Step 2 — LiveStrategyHost の単体テスト (§2.2 / §1.1 / §1.2 / §1.3)。

host のライフサイクル + 所有権/単一 run 制約 + 戦略ロードを、Fake な session /
engine controller / loader を注入して検証する。Nautilus 実エンジンは Step 3+ で
結線するため、ここでは seam（LiveEngineController）が正しく駆動されることを確認する。
"""

from __future__ import annotations

import pytest

from engine.live.run_registry import RunRegistry
from engine.live.strategy_host import (
    LiveStrategyHost,
    LiveStrategyHostError,
    StartParams,
)
from engine.live.strategy_state_machine import (
    ERROR,
    PAUSED,
    RUNNING,
    STOPPED,
)


# ── fakes ────────────────────────────────────────────────────────────────


class FakeSession:
    def __init__(self, is_logged_in: bool = True) -> None:
        self._logged_in = is_logged_in

    @property
    def is_logged_in(self) -> bool:
        return self._logged_in


class FakeController:
    """attach/detach/cancel の呼び出しを記録する。fail_* で例外を仕込める。"""

    def __init__(self) -> None:
        self.attached: list[str] = []
        self.detached: list[str] = []
        self.canceled: list[str] = []
        self.fail_attach = False

    def attach(self, *, nautilus_strategy_id, **kwargs) -> None:
        if self.fail_attach:
            raise RuntimeError("boom")
        self.attached.append(nautilus_strategy_id)

    def detach(self, *, nautilus_strategy_id) -> None:
        self.detached.append(nautilus_strategy_id)

    def cancel_inflight_orders(self, *, nautilus_strategy_id) -> None:
        self.canceled.append(nautilus_strategy_id)


_SCENARIO = {"instrument": "1301.TSE", "granularity": "Daily", "initial_cash": 1_000_000}


def _loader_ok(path):
    return (object(), _SCENARIO, type("FakeStrat", (), {}))


def _loader_raises(path):
    raise ImportError("bad strategy file")


def _make_host(
    *,
    session=None,
    controller=None,
    registry=None,
    loader=_loader_ok,
    run_ids=("run0123456789",),
):
    it = iter(run_ids)
    return LiveStrategyHost(
        run_registry=registry or RunRegistry(),
        session_provider=lambda: session if session is not None else FakeSession(),
        engine_controller=controller or FakeController(),
        loader=loader,
        run_id_factory=lambda: next(it),
        now_ms=lambda: 1_700_000_000_000,
    )


def _params(strategy_id="S1", instrument_id="1301.TSE", venue="TACHIBANA"):
    return StartParams(
        strategy_id=strategy_id,
        strategy_file="/strat/mean_reversion_01.py",
        instrument_id=instrument_id,
        venue=venue,
        params={"window": "20"},
    )


# ── start: preconditions ───────────────────────────────────────────────────


def test_start_without_session_rejects_venue_login_required():
    reg = RunRegistry()
    ctrl = FakeController()
    # session_provider returns None → no live session at all
    host = LiveStrategyHost(
        run_registry=reg,
        session_provider=lambda: None,
        engine_controller=ctrl,
        loader=_loader_ok,
    )
    with pytest.raises(LiveStrategyHostError) as ei:
        host.start_run(_params())
    assert ei.value.error_code == "VENUE_LOGIN_REQUIRED"
    assert reg.list_active() == []  # nothing registered


def test_start_with_logged_out_session_rejects_venue_login_required():
    reg = RunRegistry()
    ctrl = FakeController()
    host = _make_host(session=FakeSession(is_logged_in=False), controller=ctrl, registry=reg)
    with pytest.raises(LiveStrategyHostError) as ei:
        host.start_run(_params())
    assert ei.value.error_code == "VENUE_LOGIN_REQUIRED"
    assert ctrl.attached == []  # never touched the engine


# ── start: happy path ───────────────────────────────────────────────────────


def test_start_happy_path_attaches_and_registers_running():
    reg = RunRegistry()
    ctrl = FakeController()
    host = _make_host(controller=ctrl, registry=reg, run_ids=("abcd1234ef",))
    rec = host.start_run(_params())

    assert rec.run_id == "abcd1234ef"
    assert rec.nautilus_strategy_id == "LIVE-abcd1234"
    assert rec.strategy_id == "S1"
    assert rec.instrument_id == "1301.TSE"
    assert rec.venue == "TACHIBANA"
    assert rec.state_machine.current == RUNNING
    assert rec.state_machine.is_running is True
    # engine attached exactly once with the run's StrategyId
    assert ctrl.attached == ["LIVE-abcd1234"]
    # registered + occupies the single active slot
    assert reg.get("abcd1234ef") is rec
    assert [r.run_id for r in reg.list_active()] == ["abcd1234ef"]


def test_start_passes_loaded_strategy_and_internal_inputs_to_controller():
    captured = {}

    class CapturingController(FakeController):
        def attach(self, **kwargs):
            captured.update(kwargs)
            super().attach(nautilus_strategy_id=kwargs["nautilus_strategy_id"])

    strat_cls = type("MyStrat", (), {})

    def loader(path):
        return (object(), _SCENARIO, strat_cls)

    host = _make_host(controller=CapturingController(), loader=loader)
    host.start_run(_params(instrument_id="7203.TSE", venue="KABU"))

    assert captured["strategy_cls"] is strat_cls
    assert captured["scenario"] is _SCENARIO
    assert captured["instrument_id"] == "7203.TSE"
    assert captured["venue"] == "KABU"
    assert captured["params"] == {"window": "20"}
    assert captured["nautilus_strategy_id"].startswith("LIVE-")


# ── start: failures ─────────────────────────────────────────────────────────


def test_start_load_failure_does_not_register_or_attach():
    reg = RunRegistry()
    ctrl = FakeController()
    host = _make_host(controller=ctrl, registry=reg, loader=_loader_raises)
    with pytest.raises(LiveStrategyHostError) as ei:
        host.start_run(_params())
    assert ei.value.error_code == "STRATEGY_LOAD_FAILED"
    assert reg.list_active() == []
    assert ctrl.attached == []


def test_start_second_active_run_rejected_already_running():
    reg = RunRegistry()
    host = _make_host(registry=reg, run_ids=("run1aaaaaa", "run2bbbbbb"))
    host.start_run(_params(strategy_id="S1", instrument_id="1301.TSE"))
    # a different strategy/instrument still hits the single-run cap
    with pytest.raises(LiveStrategyHostError) as ei:
        host.start_run(_params(strategy_id="S2", instrument_id="7203.TSE"))
    assert ei.value.error_code == "LIVE_STRATEGY_ALREADY_RUNNING"
    assert len(reg.list_active()) == 1


def test_start_attach_failure_rolls_back_registration():
    reg = RunRegistry()
    ctrl = FakeController()
    ctrl.fail_attach = True
    host = _make_host(controller=ctrl, registry=reg, run_ids=("run9zzzzzz",))
    with pytest.raises(LiveStrategyHostError) as ei:
        host.start_run(_params())
    assert ei.value.error_code == "STRATEGY_ATTACH_FAILED"
    # rolled back: slot freed so a retry is possible
    assert reg.list_active() == []
    assert reg.get("run9zzzzzz") is None


def test_start_after_attach_failure_can_retry():
    reg = RunRegistry()
    ctrl = FakeController()
    ctrl.fail_attach = True
    host = _make_host(controller=ctrl, registry=reg, run_ids=("first00000", "second1111"))
    with pytest.raises(LiveStrategyHostError):
        host.start_run(_params())
    ctrl.fail_attach = False
    rec = host.start_run(_params())
    assert rec.run_id == "second1111"
    assert rec.state_machine.current == RUNNING


# ── pause / resume ──────────────────────────────────────────────────────────


def test_pause_then_resume_toggles_order_gate():
    host = _make_host(run_ids=("runpr00000",))
    rec = host.start_run(_params())
    host.pause_run("runpr00000")
    assert rec.state_machine.current == PAUSED
    assert rec.state_machine.is_running is False  # new orders gated (§1.2)
    assert rec.state_machine.is_active is True
    host.resume_run("runpr00000")
    assert rec.state_machine.current == RUNNING
    assert rec.state_machine.is_running is True


def test_pause_unknown_run_raises():
    host = _make_host()
    with pytest.raises(LiveStrategyHostError) as ei:
        host.pause_run("nope")
    assert ei.value.error_code == "UNKNOWN_RUN"


def test_double_pause_is_structured_error_not_500():
    """illegal transition は InvalidLiveStrategyTransition ではなく structured error。"""
    host = _make_host(run_ids=("rundbl0000",))
    host.start_run(_params())
    host.pause_run("rundbl0000")
    with pytest.raises(LiveStrategyHostError) as ei:
        host.pause_run("rundbl0000")  # PAUSED → PAUSED は不正遷移
    assert ei.value.error_code == "INVALID_LIVE_STRATEGY_STATE"


def test_resume_while_running_is_structured_error():
    host = _make_host(run_ids=("runres0000",))
    host.start_run(_params())  # RUNNING
    with pytest.raises(LiveStrategyHostError) as ei:
        host.resume_run("runres0000")  # RUNNING → RUNNING は不正遷移
    assert ei.value.error_code == "INVALID_LIVE_STRATEGY_STATE"


def test_pause_after_stop_is_structured_error():
    host = _make_host(run_ids=("runpas0000",))
    host.start_run(_params())
    host.stop_run("runpas0000")  # STOPPED (terminal)
    with pytest.raises(LiveStrategyHostError) as ei:
        host.pause_run("runpas0000")
    assert ei.value.error_code == "INVALID_LIVE_STRATEGY_STATE"


# ── stop ────────────────────────────────────────────────────────────────────


def test_stop_cancels_inflight_and_detaches_then_stopped():
    reg = RunRegistry()
    ctrl = FakeController()
    host = _make_host(controller=ctrl, registry=reg, run_ids=("runstop000",))
    host.start_run(_params())
    rec = host.stop_run("runstop000")
    assert rec.state_machine.current == STOPPED
    assert ctrl.canceled == ["LIVE-runstop0"]  # only this StrategyId (§1.3)
    assert ctrl.detached == ["LIVE-runstop0"]
    # terminal run frees the active slot (RunRegistry filters terminal)
    assert reg.list_active() == []


def test_stop_from_paused_is_allowed():
    host = _make_host(run_ids=("runps00000",))
    rec = host.start_run(_params())
    host.pause_run("runps00000")
    host.stop_run("runps00000")
    assert rec.state_machine.current == STOPPED


def test_stop_is_idempotent():
    ctrl = FakeController()
    host = _make_host(controller=ctrl, run_ids=("runidem000",))
    host.start_run(_params())
    host.stop_run("runidem000")
    host.stop_run("runidem000")  # no raise, no double teardown
    assert ctrl.canceled == ["LIVE-runidem0"]
    assert ctrl.detached == ["LIVE-runidem0"]


def test_stop_unknown_run_raises():
    host = _make_host()
    with pytest.raises(LiveStrategyHostError) as ei:
        host.stop_run("nope")
    assert ei.value.error_code == "UNKNOWN_RUN"


# ── fail (ERROR path) ───────────────────────────────────────────────────────


def test_fail_run_errors_then_cancels_and_stops():
    reg = RunRegistry()
    ctrl = FakeController()
    host = _make_host(controller=ctrl, registry=reg, run_ids=("runfail000",))
    rec = host.start_run(_params())
    host.fail_run("runfail000", "MAX_DAILY_LOSS_EXCEEDED")
    # ERROR is recorded then the run is driven to STOPPED (§1.3)
    assert rec.state_machine.current == STOPPED
    assert rec.state_machine.error_code == "MAX_DAILY_LOSS_EXCEEDED"
    assert ctrl.canceled == ["LIVE-runfail0"]
    assert ctrl.detached == ["LIVE-runfail0"]
    assert reg.list_active() == []


def test_fail_after_stop_is_noop():
    host = _make_host(run_ids=("runfs00000",))
    rec = host.start_run(_params())
    host.stop_run("runfs00000")
    host.fail_run("runfs00000", "LATE")  # terminal → noop
    assert rec.state_machine.current == STOPPED
    assert rec.state_machine.error_code is None


def test_failed_slot_frees_for_new_run():
    reg = RunRegistry()
    host = _make_host(registry=reg, run_ids=("runa000000", "runb000000"))
    host.start_run(_params())
    host.fail_run("runa000000", "STRATEGY_EXCEPTION")
    # slot freed → a fresh run can start
    rec = host.start_run(_params())
    assert rec.run_id == "runb000000"
    assert rec.state_machine.current == RUNNING
