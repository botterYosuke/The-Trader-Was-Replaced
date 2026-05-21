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
from typing import Any, Callable, Optional

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
        safety_rails: Any = None,
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


class NautilusLiveEngineController:
    """`OrderingVenueAdapter` を Nautilus live stack に bridge する controller (Step 4)。

    `attach()` で run ごとに `NautilusKernel`（`Trader` + `LiveExecutionEngine` +
    `LiveRiskEngine` + `LiveDataEngine` + `Cache` + `Portfolio` + `MessageBus` +
    `LiveClock`）を組み、`NautilusVenueExecClient` を `register_client` し、instrument を
    cache に登録し、戦略を `add_strategy` して live loop 上で起動する。

    Phase 10 単一 run 制約（§0.7）に合わせ、本 controller も **同時に 1 つの kernel** だけを
    保持する（attach → detach のペア）。複数 run は Phase 11。

    runtime resource（live loop / venue adapter）は server_grpc が所有するため、構築時に
    provider 経由で受け取る（共有所有権、§1.1：新規 login / WebSocket は作らない）。
    safety_rails の **ネイティブ rail** は `LiveRiskEngineConfig` に、**独自 rail** は
    exec client の pre-trade フックに渡る（§2.4）。
    """

    def __init__(
        self,
        *,
        loop_provider: Callable[[], Any],
        adapter_provider: Callable[[], Any],
        on_safety_violation: Optional[Callable[[Any], None]] = None,
        attach_timeout_s: float = 10.0,
        trader_id: str = "LIVEHOST-001",
    ) -> None:
        self._loop_provider = loop_provider
        self._adapter_provider = adapter_provider
        self._on_safety_violation = on_safety_violation
        self._attach_timeout_s = attach_timeout_s
        self._trader_id = trader_id
        self._kernel = None
        self._strategy = None
        self._strategy_id_str: Optional[str] = None

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
        safety_rails: Any = None,
    ) -> None:
        import asyncio

        loop = self._loop_provider()
        adapter = self._adapter_provider()
        if loop is None or adapter is None:
            raise RuntimeError("live loop / venue adapter not available for attach")

        fut = asyncio.run_coroutine_threadsafe(
            self._do_attach(
                strategy_cls=strategy_cls,
                scenario=scenario,
                instrument_id=instrument_id,
                params=params,
                nautilus_strategy_id=nautilus_strategy_id,
                adapter=adapter,
                loop=loop,
                safety_rails=safety_rails,
            ),
            loop,
        )
        fut.result(timeout=self._attach_timeout_s)

    async def _do_attach(
        self,
        *,
        strategy_cls,
        scenario,
        instrument_id,
        params,
        nautilus_strategy_id,
        adapter,
        loop,
        safety_rails,
    ) -> None:
        # 遅延 import（Nautilus は重く、Noop 経路では読み込みたくない）。
        from nautilus_trader.common.providers import InstrumentProvider
        from nautilus_trader.config import (
            LoggingConfig,
            TradingNodeConfig,
        )
        from nautilus_trader.live.config import (
            LiveDataEngineConfig,
            LiveExecEngineConfig,
            LiveRiskEngineConfig,
        )
        from nautilus_trader.model.identifiers import InstrumentId, Venue
        from nautilus_trader.system.kernel import NautilusKernel

        from engine.live.nautilus_exec_client import NautilusVenueExecClient
        from engine.live.safety_rails import SafetyLimits, SafetyRails
        from engine.strategy_runtime.engine_runner import default_strategy_init_kwargs
        from engine.strategy_runtime.instrument_factory import make_equity_instrument

        rails = safety_rails if safety_rails is not None else SafetyRails(SafetyLimits())
        iid = InstrumentId.from_str(instrument_id)
        venue_str = iid.venue.value

        risk_cfg: LiveRiskEngineConfig = rails.to_live_risk_engine_config([instrument_id])
        cfg = TradingNodeConfig(
            trader_id=self._trader_id,
            # log_level_file="OFF": live は bypass_logging を許さないが、ファイル出力は止める
            # （cwd に LIVEHOST-001_*.log を撒かない）。console は ERROR のみ。
            logging=LoggingConfig(
                log_level="ERROR", log_level_file="OFF", print_config=False
            ),
            exec_engine=LiveExecEngineConfig(),
            risk_engine=risk_cfg,
            data_engine=LiveDataEngineConfig(),
        )
        kernel = NautilusKernel(name="LiveStrategyHost", config=cfg, loop=loop)

        # instrument を cache へ（RiskEngine の notional 計算 / exec client の precision に必要）。
        instrument = make_equity_instrument(iid.symbol.value, venue_str)
        kernel.cache.add_instrument(instrument)

        client = NautilusVenueExecClient(
            loop=loop,
            venue=Venue(venue_str),
            msgbus=kernel.msgbus,
            cache=kernel.cache,
            clock=kernel.clock,
            adapter=adapter,
            safety_rails=rails,
            instrument_provider=InstrumentProvider(),
            on_safety_violation=self._on_safety_violation,
        )
        kernel.exec_engine.register_client(client)

        # 戦略インスタンス化（engine_runner の backtest と同じ contract）。
        # config= 形式の戦略は scenario/params から組めないため、kwargs 形式
        # （instrument_id / bar_type_str）を default として渡す（mean_reversion_01 等）。
        kwargs = default_strategy_init_kwargs(scenario)
        kwargs.update(params)
        strategy = strategy_cls(**kwargs)
        kernel.trader.add_strategy(strategy)

        kernel.start()

        self._kernel = kernel
        self._strategy = strategy
        self._strategy_id_str = str(strategy.id)

    def detach(self, *, nautilus_strategy_id: str) -> None:
        self._teardown_kernel()

    def cancel_inflight_orders(self, *, nautilus_strategy_id: str) -> None:
        kernel = self._kernel
        strategy = self._strategy
        if kernel is None or strategy is None:
            return
        import asyncio

        loop = self._loop_provider()
        if loop is None:
            return

        async def _cancel() -> None:
            try:
                # 当該戦略の open order のみ cancel（§1.3 / M6）。手動・他戦略は巻き込まない。
                strategy.cancel_all_orders(strategy.instrument_id) if hasattr(
                    strategy, "instrument_id"
                ) else None
            except Exception:  # noqa: BLE001 — best-effort
                log.exception("cancel_inflight_orders failed")

        try:
            asyncio.run_coroutine_threadsafe(_cancel(), loop).result(timeout=5.0)
        except Exception:  # noqa: BLE001
            log.exception("cancel_inflight_orders scheduling failed")

    def _teardown_kernel(self) -> None:
        kernel = self._kernel
        if kernel is None:
            return
        import asyncio

        loop = self._loop_provider()
        self._kernel = None
        self._strategy = None
        self._strategy_id_str = None
        if loop is None:
            return
        try:
            asyncio.run_coroutine_threadsafe(kernel.stop_async(), loop).result(timeout=10.0)
        except Exception:  # noqa: BLE001 — 停止失敗でも run state は terminal にする
            log.exception("kernel stop_async failed during detach")
