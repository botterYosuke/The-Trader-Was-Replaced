"""engine.live.strategy_state_machine — Live 戦略 run の状態機械 (Phase 10 §1.2)。

    IDLE → LOADING → READY → RUNNING → (PAUSED) → STOPPING → STOPPED
                                  ↘ ERROR (safety rail violation / venue error)

- `READY`: ロード済み・Safety Rails 設定済み・まだ market data を流していない。
- `RUNNING` ↔ `PAUSED`: Pause は「新規発注ゲート」(§1.2 / M5)。callback は継続し得るが
  `is_running` が False の間は host/safety_rails が新規注文を deny する。
- `ERROR`: safety rail 違反 / venue error / 戦略例外。host が内部 StopLiveStrategy を
  発射して当該 StrategyId の in-flight order を cancel した後、`STOPPED` に落とす (§1.3)。

`engine.live.state_machine.VenueStateMachine`（venue 接続用）とは別物。
"""

from __future__ import annotations

IDLE = "IDLE"
LOADING = "LOADING"
READY = "READY"
RUNNING = "RUNNING"
PAUSED = "PAUSED"
STOPPING = "STOPPING"
STOPPED = "STOPPED"
ERROR = "ERROR"

_STATES: frozenset[str] = frozenset(
    {IDLE, LOADING, READY, RUNNING, PAUSED, STOPPING, STOPPED, ERROR}
)

_ALLOWED: dict[str, set[str]] = {
    IDLE: {LOADING},
    LOADING: {READY, ERROR},
    READY: {RUNNING, STOPPING, ERROR},
    RUNNING: {PAUSED, STOPPING, ERROR},
    PAUSED: {RUNNING, STOPPING, ERROR},
    STOPPING: {STOPPED, ERROR},
    ERROR: {STOPPED},
    STOPPED: set(),
}


class InvalidLiveStrategyTransition(Exception):
    """不正な状態遷移が要求されたとき。"""


class LiveStrategyStateMachine:
    def __init__(self) -> None:
        self.current: str = IDLE
        self.error_code: str | None = None

    def transition_to(self, target: str) -> None:
        if target not in _STATES:
            raise InvalidLiveStrategyTransition(f"unknown target state: {target!r}")
        if target not in _ALLOWED[self.current]:
            raise InvalidLiveStrategyTransition(
                f"illegal transition: {self.current} -> {target}"
            )
        self.current = target

    def error(self, error_code: str) -> None:
        """ERROR 状態へ遷移し error_code を記録する（terminal からは不可）。"""
        self.transition_to(ERROR)
        self.error_code = error_code

    @property
    def is_running(self) -> bool:
        """新規発注を受け付ける状態か（RUNNING のみ。PAUSED は deny, §1.2）。"""
        return self.current == RUNNING

    @property
    def is_active(self) -> bool:
        """run が稼働中か（RUNNING または PAUSED）。"""
        return self.current in (RUNNING, PAUSED)

    @property
    def is_terminal(self) -> bool:
        return self.current == STOPPED
