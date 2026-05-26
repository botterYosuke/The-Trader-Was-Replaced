"""engine.live.strategy_host — Live 戦略 run のホスト (Phase 10 §2.2 / Step 2)。

`LiveStrategyHost` は「Replay で検証した `Strategy` を Live Auto で起動する」経路の
オーケストレーション層。担うのは **ライフサイクル管理 + 所有権/単一 run 制約 +
戦略ロード** であって、Nautilus live engine（`Trader` + `LiveDataEngine` +
`LiveExecutionEngine` + `LiveRiskEngine`）への attach/detach や in-flight 注文の
cancel は注入された `LiveEngineController`（Nautilus seam）に委譲する。

設計の根拠（計画書 §0′ / §1.1 / §2.2）:
- **session は共有する（二重 login しない）**: 既存 Phase 9 live session
  (`server_grpc._live_runner` / `_order_facade` / adapter) を単一所有者として
  借用する。host は `session_provider()` で現在の live session を取得し、未ログイン
  なら `VENUE_LOGIN_REQUIRED` で reject する。新しい login / WebSocket / order client
  は作らない。
- **transport 非依存**: proto を import しない（ManualOrderFacade と同方針）。
  proto 変換・`token`/`ExecutionMode` 検証は gRPC handler (§2.5 / Step 3) の責務。
- **戦略のインスタンス化は engine 層の責務**: `strategy_loader.load()` は
  `(module, scenario, strategy_cls)` を返すだけ。`strategy_cls(**kwargs)` 化と
  EXTERNAL→INTERNAL `bar_type` 読み替え（`bar_supply.to_internal_bar_type`）は
  `LiveEngineController.attach()` 側で行う（engine_runner が backtest で
  `strategy_cls(**kwargs); engine.add_strategy()` するのと同じ分担）。

`strategy_id` = `RegisterLiveStrategy`（Step 3 / §2.5）が検証済みファイルに発行する
opaque handle。Step 2 の host は file path を直接受け取り、strategy_id↔file の解決と
path 検証は gRPC layer に委ねる。
`nautilus_strategy_id` = `LIVE-{run_id 短縮}`。発注主体識別（§2.9 / M6）に使う。
"""

from __future__ import annotations

import logging
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Optional, Protocol

from engine.live.run_registry import (
    DuplicateStrategyInstrument,
    LiveStrategyAlreadyRunning,
    RunRecord,
    RunRegistry,
)
from engine.live.strategy_state_machine import (
    ERROR,
    InvalidLiveStrategyTransition,
    LiveStrategyStateMachine,
    PAUSED,
    READY,
    RUNNING,
    STOPPED,
    STOPPING,
)
from engine.strategy_runtime import strategy_loader


class LiveStrategyHostError(Exception):
    """host レベルの既知エラー。`error_code` を gRPC Res にそのまま載せる。

    error_code は構造的に意味が決まっている文字列（VENUE_LOGIN_REQUIRED /
    STRATEGY_LOAD_FAILED / LIVE_STRATEGY_ALREADY_RUNNING / DUPLICATE_STRATEGY_INSTRUMENT /
    STRATEGY_ATTACH_FAILED / UNKNOWN_RUN / INVALID_LIVE_STRATEGY_STATE）。
    """

    def __init__(self, error_code: str) -> None:
        super().__init__(error_code)
        self.error_code = error_code


class LiveSessionView(Protocol):
    """host が借用する現在の live session のビュー（共有所有権）。

    server_grpc の `_live_runner` / adapter から薄く構成する。host はこの read-only
    ビューだけを見て二重 login しない。
    """

    @property
    def is_logged_in(self) -> bool: ...  # noqa: E704


class LiveEngineController(Protocol):
    """Nautilus live engine への attach/detach/cancel を抽象化する seam。

    Step 2 の host はこの Protocol だけに依存する。実体（`Trader` +
    `LiveDataEngine` + `LiveExecutionEngine` + `LiveRiskEngine` を既存 adapter に
    bridge する controller）は Step 3 以降で結線し、gRPC layer が注入する。
    テストは Fake controller を注入して host のライフサイクルを検証する。
    """

    def attach(
        self,
        *,
        strategy_cls: Any,
        scenario: dict,
        instrument_id: str,
        venue: str,
        params: dict[str, str],
        nautilus_strategy_id: str,
        session: LiveSessionView,
        safety_rails: Any = None,
    ) -> None:
        """`strategy_cls` をインスタンス化し `Trader.add_strategy()` で attach する。

        EXTERNAL→INTERNAL の `bar_type` 読み替え（§2.3）と strategy ctor 形式
        （config= / kwargs）の吸収はこの実装の責務。`safety_rails`（§2.4）は
        `LiveRiskEngineConfig`（ネイティブ rail）と exec client の独自 pre-trade フックに渡す。
        """
        ...

    def detach(self, *, nautilus_strategy_id: str) -> None:
        """attach 済み戦略を Trader から取り外す（`Trader.remove_strategy()` 相当）。"""
        ...

    def cancel_inflight_orders(self, *, nautilus_strategy_id: str) -> None:
        """当該 `StrategyId` の in-flight order **のみ** cancel する（§1.3 / M6）。

        手動発注（`MANUAL-001`）や他戦略の注文を巻き込まないこと。
        """
        ...


@dataclass(frozen=True)
class StartParams:
    """`StartLiveStrategy`（§2.5）の host 入力。proto から gRPC layer が組む。"""

    strategy_id: str
    strategy_file: str
    instrument_id: str
    venue: str
    params: dict[str, str] = field(default_factory=dict)
    # §2.4 Safety Rails。engine controller の attach に素通しする（host は中身を見ない、
    # transport 非依存）。None なら controller 側で「全 rail 無効」の既定にフォールバック。
    safety_rails: Any = None


def _new_run_id() -> str:
    return uuid.uuid4().hex


def _now_ms() -> int:
    return int(time.time() * 1000)


class LiveStrategyHost:
    def __init__(
        self,
        *,
        run_registry: RunRegistry,
        session_provider: Callable[[], Optional[LiveSessionView]],
        engine_controller: LiveEngineController,
        loader: Callable[[Any], tuple] = strategy_loader.load,
        run_id_factory: Callable[[], str] = _new_run_id,
        now_ms: Callable[[], int] = _now_ms,
    ) -> None:
        self._registry = run_registry
        self._session_provider = session_provider
        self._controller = engine_controller
        self._loader = loader
        self._run_id_factory = run_id_factory
        self._now_ms = now_ms

    # ── lifecycle ──────────────────────────────────────────────────────────

    def start_run(self, params: StartParams) -> RunRecord:
        """戦略を Live Auto で起動する。READY 経由で RUNNING まで進めて record を返す。

        Raises:
            LiveStrategyHostError: precondition / load / 単一run制約 / attach 失敗。
        """
        # (1) precondition: 既存 live session が logged-in であること（共有所有権）。
        #     未ログインで自前 login はしない（§1.1 ⚠️ session 所有権）。
        session = self._session_provider()
        if session is None or not session.is_logged_in:
            raise LiveStrategyHostError("VENUE_LOGIN_REQUIRED")

        sm = LiveStrategyStateMachine()
        sm.transition_to("LOADING")

        # (2) 戦略ロード（インスタンス化はしない、§0.4）。
        try:
            _module, scenario, strategy_cls = self._loader(params.strategy_file)
        except Exception as exc:  # noqa: BLE001 — load 失敗は構造化エラーに正規化
            sm.error("STRATEGY_LOAD_FAILED")
            raise LiveStrategyHostError("STRATEGY_LOAD_FAILED") from exc

        sm.transition_to(READY)

        # (3) id 採番 + RunRegistry 登録（単一 run 制約 / 重複検出をここで強制）。
        #     attach の **前** に登録してスロットを予約する。登録が reject されたら
        #     engine には一切触れない（二重発注の芽を断つ）。
        run_id = self._run_id_factory()
        nautilus_strategy_id = f"LIVE-{run_id[:8]}"
        try:
            record = self._registry.register(
                run_id=run_id,
                strategy_id=params.strategy_id,
                instrument_id=params.instrument_id,
                nautilus_strategy_id=nautilus_strategy_id,
                venue=params.venue,
                started_ts_ms=self._now_ms(),
                state_machine=sm,
            )
        except LiveStrategyAlreadyRunning as exc:
            sm.error("LIVE_STRATEGY_ALREADY_RUNNING")
            raise LiveStrategyHostError("LIVE_STRATEGY_ALREADY_RUNNING") from exc
        except DuplicateStrategyInstrument as exc:
            sm.error("DUPLICATE_STRATEGY_INSTRUMENT")
            raise LiveStrategyHostError("DUPLICATE_STRATEGY_INSTRUMENT") from exc

        # (4) Nautilus engine に attach（seam）。失敗したら登録を巻き戻して ERROR。
        try:
            self._controller.attach(
                strategy_cls=strategy_cls,
                scenario=scenario,
                instrument_id=params.instrument_id,
                venue=params.venue,
                params=dict(params.params),
                nautilus_strategy_id=nautilus_strategy_id,
                session=session,
                safety_rails=params.safety_rails,
            )
        except Exception as exc:  # noqa: BLE001
            logging.exception("Live strategy attach failed")
            self._registry.unregister(run_id)
            sm.error("STRATEGY_ATTACH_FAILED")
            raise LiveStrategyHostError("STRATEGY_ATTACH_FAILED") from exc

        # (5) RUNNING へ。Strategy の on_start は Nautilus が attach 時に呼ぶ。
        sm.transition_to(RUNNING)
        return record

    def pause_run(self, run_id: str) -> RunRecord:
        """RUNNING → PAUSED（新規発注ゲートを閉じる、§1.2）。callback は継続し得る。"""
        record = self._require_run(run_id)
        self._guarded_transition(record, PAUSED)
        return record

    def resume_run(self, run_id: str) -> RunRecord:
        """PAUSED → RUNNING。"""
        record = self._require_run(run_id)
        self._guarded_transition(record, RUNNING)
        return record

    def stop_run(self, run_id: str) -> RunRecord:
        """graceful 停止。detach + 当該 StrategyId の in-flight cancel → STOPPED。

        既に STOPPED なら冪等に record を返す。READY/RUNNING/PAUSED からは
        STOPPING を経由、ERROR からは直接 STOPPED（state machine の許可遷移に従う）。
        在庫ポジションは残す（§0.3、ユーザー判断で別途決済）。
        """
        record = self._require_run(run_id)
        sm = record.state_machine
        if sm.is_terminal:
            return record
        if sm.current != ERROR:
            sm.transition_to(STOPPING)
        self._teardown(record)
        sm.transition_to(STOPPED)
        return record

    def fail_run(self, run_id: str, error_code: str) -> RunRecord:
        """safety rail 違反 / venue error / 戦略例外で ERROR へ落とし、内部停止する。

        ERROR → 当該 StrategyId の in-flight cancel + detach → STOPPED（§1.3）。
        手動 / 他戦略の注文は巻き込まない。
        """
        record = self._require_run(run_id)
        sm = record.state_machine
        if sm.is_terminal:
            return record
        sm.error(error_code)
        self._teardown(record)
        sm.transition_to(STOPPED)
        return record

    # ── helpers ────────────────────────────────────────────────────────────

    def _require_run(self, run_id: str) -> RunRecord:
        record = self._registry.get(run_id)
        if record is None:
            raise LiveStrategyHostError("UNKNOWN_RUN")
        return record

    @staticmethod
    def _guarded_transition(record: RunRecord, target: str) -> None:
        """状態機械の不正遷移を構造化エラーに正規化する。

        double-pause / resume-while-running / pause-after-stop 等の不正要求を
        `InvalidLiveStrategyTransition`（→ gRPC 500）ではなく
        `INVALID_LIVE_STRATEGY_STATE`（structured `error_code`）で返す。
        """
        try:
            record.state_machine.transition_to(target)
        except InvalidLiveStrategyTransition as exc:
            raise LiveStrategyHostError("INVALID_LIVE_STRATEGY_STATE") from exc

    def _teardown(self, record: RunRecord) -> None:
        """in-flight cancel（当該 StrategyId のみ）→ detach。best-effort で両方試みる。

        cancel が失敗しても detach は試みる（戦略を engine に残さない）。停止経路で
        例外が伝播して run が中途半端な状態に固着するのを防ぐ。
        """
        nsid = record.nautilus_strategy_id
        try:
            self._controller.cancel_inflight_orders(nautilus_strategy_id=nsid)
        finally:
            self._controller.detach(nautilus_strategy_id=nsid)
