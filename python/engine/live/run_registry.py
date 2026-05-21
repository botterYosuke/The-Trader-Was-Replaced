"""engine.live.run_registry — Live 戦略 run の in-memory 管理 (Phase 10 §2.6)。

- Phase 10 MVP: automated Live run は同時 1 件 (`max_active_live_auto_runs = 1`)。
  既に非終端 run があれば新規登録を `LiveStrategyAlreadyRunning` で reject する (§0.7)。
- 将来拡張 (Phase 11) 用に `(strategy_id, instrument_id)` 索引を持ち、同じ戦略を
  同じ銘柄で二重起動することを `DuplicateStrategyInstrument` で防ぐ (M4)。
- 各 run は一意な Nautilus `StrategyId` を保持し、`OrderEvent.strategy_id` → run_id の
  逆引き（発注主体識別, §2.9 / M6）に使う。
- 永続化なし。プロセス再起動で全 run が消える（venue 側に注文が残る可能性は UI 警告）。

スレッド安全性: gRPC handler は threadpool で並行する。RunRegistry はホスト（write）と
読み取り handler（`GetLiveStrategyStatus` / `ListLiveStrategies`）から同時に触られ、
将来は engine スレッドからも fail/stop される。よって **内部 lock で全 method を保護**し、
単一 run スロットの check-and-set も 1 つのクリティカルセクションで原子的に行う
（外側の lock 任せにしない）。

終端 (STOPPED) run は post-stop の `GetLiveStrategyStatus` のために保持する。退避は
register 時に行い、終端 run を新しい順に `max_terminal_history` 件だけ残す。レコードが
増えるのは register だけ（stop/fail は active→terminal に変えるだけで件数は増えない）で、
active run は `max_active` 件までなので、総レコード数は常に
`max_terminal_history + max_active` で上限が押さえられる（無制限には増えない）。

`strategy_id` = `RegisterLiveStrategy` 発行の opaque handle。
`nautilus_strategy_id` = `LIVE-{run_id 短縮}` 等の Nautilus `StrategyId`（発注主体）。
"""

from __future__ import annotations

import threading
from dataclasses import dataclass

from engine.live.strategy_state_machine import LiveStrategyStateMachine


class LiveStrategyAlreadyRunning(Exception):
    """active automated run 上限を超えて登録しようとしたとき (§0.7)。"""


class DuplicateStrategyInstrument(Exception):
    """同じ (strategy_id, instrument_id) の run が既に存在するとき (M4)。"""


@dataclass
class RunRecord:
    run_id: str
    strategy_id: str
    instrument_id: str
    nautilus_strategy_id: str
    venue: str
    started_ts_ms: int
    state_machine: LiveStrategyStateMachine


class RunRegistry:
    def __init__(
        self,
        max_active_live_auto_runs: int = 1,
        max_terminal_history: int = 64,
    ) -> None:
        self._max_active = max_active_live_auto_runs
        self._max_terminal_history = max_terminal_history
        self._runs: dict[str, RunRecord] = {}
        self._lock = threading.Lock()

    def register(
        self,
        *,
        run_id: str,
        strategy_id: str,
        instrument_id: str,
        nautilus_strategy_id: str,
        venue: str,
        started_ts_ms: int,
        state_machine: LiveStrategyStateMachine,
    ) -> RunRecord:
        with self._lock:
            active = self._active_locked()
            if len(active) >= self._max_active:
                raise LiveStrategyAlreadyRunning(
                    f"active automated run limit reached ({self._max_active}); "
                    f"running={[r.run_id for r in active]}"
                )
            for rec in active:
                if (rec.strategy_id, rec.instrument_id) == (strategy_id, instrument_id):
                    raise DuplicateStrategyInstrument(
                        f"({strategy_id}, {instrument_id}) already running as {rec.run_id}"
                    )

            record = RunRecord(
                run_id=run_id,
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                nautilus_strategy_id=nautilus_strategy_id,
                venue=venue,
                started_ts_ms=started_ts_ms,
                state_machine=state_machine,
            )
            self._runs[run_id] = record
            self._evict_terminal_locked()
            return record

    def unregister(self, run_id: str) -> bool:
        """run を登録解除する。存在しなければ False。"""
        with self._lock:
            return self._runs.pop(run_id, None) is not None

    def get(self, run_id: str) -> RunRecord | None:
        with self._lock:
            return self._runs.get(run_id)

    def run_id_for_nautilus_strategy(self, nautilus_strategy_id: str) -> str | None:
        with self._lock:
            for rec in self._runs.values():
                if rec.nautilus_strategy_id == nautilus_strategy_id:
                    return rec.run_id
        return None

    def list_active(self) -> list[RunRecord]:
        """非終端（STOPPED でない）run の一覧。スロット占有判定にも使う。"""
        with self._lock:
            return self._active_locked()

    # ── internal (lock held by caller) ──────────────────────────────────────

    def _active_locked(self) -> list[RunRecord]:
        """呼び出し元が self._lock を保持していること。"""
        return [r for r in self._runs.values() if not r.state_machine.is_terminal]

    def _evict_terminal_locked(self) -> None:
        """終端 run が上限を超えたら古い順（started_ts_ms 昇順）に退避する。

        呼び出し元が self._lock を保持していること。
        """
        terminal = [r for r in self._runs.values() if r.state_machine.is_terminal]
        overflow = len(terminal) - self._max_terminal_history
        if overflow <= 0:
            return
        terminal.sort(key=lambda r: r.started_ts_ms)
        for rec in terminal[:overflow]:
            self._runs.pop(rec.run_id, None)
