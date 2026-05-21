"""engine.live.run_registry — Live 戦略 run の in-memory 管理 (Phase 10 §2.6)。

- Phase 10 MVP: automated Live run は同時 1 件 (`max_active_live_auto_runs = 1`)。
  既に非終端 run があれば新規登録を `LiveStrategyAlreadyRunning` で reject する (§0.7)。
- 将来拡張 (Phase 11) 用に `(strategy_id, instrument_id)` 索引を持ち、同じ戦略を
  同じ銘柄で二重起動することを `DuplicateStrategyInstrument` で防ぐ (M4)。
- 各 run は一意な Nautilus `StrategyId` を保持し、`OrderEvent.strategy_id` → run_id の
  逆引き（発注主体識別, §2.9 / M6）に使う。
- 永続化なし。プロセス再起動で全 run が消える（venue 側に注文が残る可能性は UI 警告）。

`strategy_id` = `RegisterLiveStrategy` 発行の opaque handle。
`nautilus_strategy_id` = `LIVE-{run_id 短縮}` 等の Nautilus `StrategyId`（発注主体）。
"""

from __future__ import annotations

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
    def __init__(self, max_active_live_auto_runs: int = 1) -> None:
        self._max_active = max_active_live_auto_runs
        self._runs: dict[str, RunRecord] = {}

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
        active = self.list_active()
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
        return record

    def unregister(self, run_id: str) -> bool:
        """run を登録解除する。存在しなければ False。"""
        return self._runs.pop(run_id, None) is not None

    def get(self, run_id: str) -> RunRecord | None:
        return self._runs.get(run_id)

    def run_id_for_nautilus_strategy(self, nautilus_strategy_id: str) -> str | None:
        for rec in self._runs.values():
            if rec.nautilus_strategy_id == nautilus_strategy_id:
                return rec.run_id
        return None

    def list_active(self) -> list[RunRecord]:
        """非終端（STOPPED でない）run の一覧。スロット占有判定にも使う。"""
        return [r for r in self._runs.values() if not r.state_machine.is_terminal]
