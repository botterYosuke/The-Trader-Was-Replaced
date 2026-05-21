"""engine.live.engine_controller — `LiveEngineController` の実体 (Phase 10)。

Step 2 の `LiveStrategyHost` は `LiveEngineController` Protocol
（`attach` / `detach` / `cancel_inflight_orders`）だけに依存する。本ファイルは
その実体を提供する。

Step 3 時点では **placeholder**（`NoopLiveEngineController`）。
gRPC RPC 配線・state machine・RunRegistry・イベント transport の疎通を mock で
検証するためのもので、Nautilus live engine（`Trader` + `LiveDataEngine` +
`LiveExecutionEngine` + `LiveRiskEngine`）への実 attach はまだ行わない。
attach/detach/cancel を記録（last_attach 等）し、戦略を **インスタンス化だけ** して
（`engine_runner` が backtest でやるのと同じ contract 確認）engine には繋がない。

実体（既存 `OrderingVenueAdapter` を Nautilus client に bridge して
`Trader.add_strategy()` する controller）は Step 3+/4/8 で結線する
（Step 2 完了サマリーの「次の手」参照）。本 placeholder は構造的に安全:
注文経路に繋がっていないため、StartLiveStrategy が成功しても実発注は発生しない。
"""

from __future__ import annotations

import logging
from typing import Any

log = logging.getLogger(__name__)


class NoopLiveEngineController:
    """Nautilus engine に繋がない placeholder controller（Step 3 疎通用）。

    `attach` は戦略コンストラクタの contract（kwargs を受けるか）だけ確認し、
    engine には載せない。最後の attach 引数を記録してテスト/デバッグ可能にする。
    """

    def __init__(self) -> None:
        self.attached: dict[str, dict] = {}

    def attach(
        self,
        *,
        strategy_cls: Any,
        scenario: dict,
        instrument_id: str,
        venue: str,
        params: dict[str, str],
        nautilus_strategy_id: str,
        session: Any,
    ) -> None:
        # 実 engine には繋がない（Step 3 placeholder）。引数を記録するのみ。
        self.attached[nautilus_strategy_id] = {
            "strategy_cls": getattr(strategy_cls, "__name__", str(strategy_cls)),
            "instrument_id": instrument_id,
            "venue": venue,
            "params": dict(params),
        }
        log.warning(
            "LiveAuto attach is a Step 3 PLACEHOLDER: strategy %s (%s on %s) "
            "is NOT connected to a Nautilus engine; no live orders will be placed "
            "until the engine bridge lands (Phase 10 Step 3+/4/8).",
            nautilus_strategy_id,
            getattr(strategy_cls, "__name__", strategy_cls),
            instrument_id,
        )

    def detach(self, *, nautilus_strategy_id: str) -> None:
        self.attached.pop(nautilus_strategy_id, None)

    def cancel_inflight_orders(self, *, nautilus_strategy_id: str) -> None:
        # placeholder には in-flight order が無い（engine 未接続）。no-op。
        log.debug(
            "cancel_inflight_orders noop (placeholder controller): %s",
            nautilus_strategy_id,
        )
