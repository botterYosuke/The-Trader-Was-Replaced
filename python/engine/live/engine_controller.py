"""engine.live.engine_controller — `LiveEngineController` の実体 (Phase 10)。

`LiveStrategyHost` は `LiveEngineController` Protocol
（`attach` / `detach` / `cancel_inflight_orders`）だけに依存する。本ファイルは
その実体を 2 つ提供する。

- **`NautilusLiveEngineController`（本番実装）**: 既存 `OrderingVenueAdapter` を
  Nautilus live stack に bridge し、`attach()` で run ごとに `NautilusKernel`
  （`Trader` + `LiveExecutionEngine` + `LiveRiskEngine` + `LiveDataEngine` + `Cache` +
  `Portfolio` + `MessageBus` + `LiveClock`）を組んで `NautilusVenueExecClient` を
  `register_client` し、戦略を `add_strategy()` して live loop 上で起動する
  （Step 4/7/8 で結線済み）。`_backend_impl` の既定 controller はこれで、StartLiveStrategy が
  成功すると当該戦略は実際に venue へ発注し得る。

- **`NoopLiveEngineController`（テスト専用 placeholder）**: Nautilus engine には繋がない。
  attach/detach/cancel を記録するのみで、戦略を **インスタンス化だけ** して
  （`engine_runner` が backtest でやるのと同じ contract 確認）engine には載せない。
  gRPC RPC 配線・state machine・RunRegistry・イベント transport の疎通を、Nautilus を
  起動せずに検証するためのもの。注文経路に繋がっていないため実発注は発生しない。
  テスト（_backend_impl 単体テスト等）が明示的に注入する。
"""

from __future__ import annotations

import logging
from typing import Any, Callable, Optional

from engine.live.strategy_log import strategy_log_topic

log = logging.getLogger(__name__)


class NoopLiveEngineController:
    """Nautilus engine に繋がない placeholder controller（テスト専用 / gRPC plumbing 疎通用）。

    本番経路は `NautilusLiveEngineController`。これはテストが注入する placeholder で、
    `attach` は戦略コンストラクタの contract（kwargs を受けるか）だけ確認し、engine には
    載せない。最後の attach 引数を記録してテスト/デバッグ可能にする。
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
        # 実 engine には繋がない（テスト専用 placeholder）。引数を記録するのみ。
        self.attached[nautilus_strategy_id] = {
            "strategy_cls": getattr(strategy_cls, "__name__", str(strategy_cls)),
            "instrument_id": instrument_id,
            "venue": venue,
            "params": dict(params),
        }
        log.warning(
            "LiveAuto attach via NoopLiveEngineController (TEST PLACEHOLDER): strategy %s "
            "(%s on %s) is NOT connected to a Nautilus engine; no live orders will be placed. "
            "Production uses NautilusLiveEngineController.",
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

    runtime resource（live loop / venue adapter）は _backend_impl が所有するため、構築時に
    provider 経由で受け取る（共有所有権、§1.1：新規 login / WebSocket は作らない）。
    safety_rails の **ネイティブ rail** は `LiveRiskEngineConfig` に、**独自 rail** は
    exec client の pre-trade フックに渡る（§2.4）。
    """

    def __init__(
        self,
        *,
        loop_provider: Callable[[], Any],
        adapter_provider: Callable[[], Any],
        runner_provider: Optional[Callable[[], Any]] = None,
        on_safety_violation: Optional[Callable[[Any], None]] = None,
        on_order_event: Optional[Callable[[Any, str], None]] = None,
        on_telemetry: Optional[Callable[[str, dict], None]] = None,
        on_strategy_log: Optional[Callable[[Any, str], None]] = None,
        run_gate_provider: Optional[Callable[[str], bool]] = None,
        attach_timeout_s: float = 10.0,
        trader_id: str = "LIVEHOST-001",
    ) -> None:
        self._loop_provider = loop_provider
        self._adapter_provider = adapter_provider
        # Step 8: live tick → Nautilus aggregation の供給源。`LiveRunner` を返す provider。
        # None（テストの直結 kernel など）の場合は tick tap を張らない。
        self._runner_provider = runner_provider
        self._on_safety_violation = on_safety_violation
        # Step 7 C: kernel msgbus 由来の OrderEvent を UI へ橋渡しする callback。
        # 署名は (OrderEventData, strategy_id) で、strategy_id は当該 run の
        # nautilus_strategy_id（"LIVE-{run}"）。_backend_impl が注入する。
        self._on_order_event = on_order_event
        # Step 7 D: run 別 telemetry を push する callback。署名は (strategy_id, metrics dict)。
        self._on_telemetry = on_telemetry
        # §570 (Step 9 remediation): strategy が `strategy.log.{strategy_id}` に publish した
        # UI ログ行を橋渡しする callback。署名は (StrategyLogRecord, strategy_id)。
        self._on_strategy_log = on_strategy_log
        # Issue #6: 当該 run（nautilus_strategy_id）が PAUSED で新規発注ゲートが閉じているかを
        # 返す provider。`(nautilus_strategy_id) -> bool`、True なら exec client が deny する。
        # _backend_impl が RunRegistry 逆引き + state_machine.is_running で構成する。
        self._run_gate_provider = run_gate_provider
        self._attach_timeout_s = attach_timeout_s
        self._trader_id = trader_id
        self._kernel = None
        self._strategy = None
        self._strategy_id_str: Optional[str] = None
        # Step 8: bar 供給用 data client と、それに tick を流す LiveRunner listener。
        self._data_client = None
        self._tick_listener: Optional[Callable[[Any], None]] = None
        self._runner = None

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
        from nautilus_trader.model.identifiers import InstrumentId, StrategyId, Venue
        from nautilus_trader.system.kernel import NautilusKernel

        from engine.live.bar_supply import live_bar_type
        from engine.live.nautilus_data_client import NautilusVenueDataClient
        from engine.live.nautilus_exec_client import NautilusVenueExecClient
        from engine.live.safety_rails import SafetyLimits, SafetyRails
        from engine.strategy_runtime.catalog_data_loader import normalize_granularity
        from engine.strategy_runtime.instrument_factory import make_equity_instrument

        rails = safety_rails if safety_rails is not None else SafetyRails(SafetyLimits())
        iid = InstrumentId.from_str(instrument_id)
        venue_str = iid.venue.value

        live_instrument_ids = self._live_instrument_ids(
            primary_instrument_id=instrument_id,
            scenario=scenario,
        )
        risk_cfg: LiveRiskEngineConfig = rails.to_live_risk_engine_config(live_instrument_ids)
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
        # `_do_attach` は live loop 上で実行される（attach() が run_coroutine_threadsafe で
        # 投げる）ため、kernel は `loop=` を渡さず `asyncio.get_running_loop()` で同じ loop に
        # bind させる。`loop=` を渡すと NautilusKernel が `_setup_loop()` で
        # `signal.signal(SIGINT)` / `loop.add_signal_handler()` を呼ぶが、これらは
        # **メインスレッドでしか動かない**。本番の live loop は _backend_impl の daemon thread
        # （phase8-live-loop）で回っており、Python 3.14 は非メインスレッドの signal.signal で
        # `ValueError: signal only works in main thread` を raise する（#36）。プロセスの
        # ライフサイクル/シグナルは backend が所有するので、Nautilus には signal handler を
        # 登録させない。sync `kernel.start()` 経路は `_register_executor()` を呼ばないため、
        # `loop=` 省略で executor が未設定でも安全（start_async() のみが executor を要求する）。
        kernel = NautilusKernel(name="LiveStrategyHost", config=cfg)

        # instruments を cache/provider へ（RiskEngine の notional 計算、exec client の
        # precision、LiveDataEngine の bar aggregation instrument lookup に必要）。
        instrument_provider = InstrumentProvider()
        instruments = []
        for live_instrument_id in live_instrument_ids:
            live_iid = InstrumentId.from_str(live_instrument_id)
            instrument = make_equity_instrument(live_iid.symbol.value, live_iid.venue.value)
            kernel.cache.add_instrument(instrument)
            instruments.append(instrument)
        instrument_provider.add_bulk(instruments)

        client = NautilusVenueExecClient(
            loop=loop,
            venue=Venue(venue_str),
            msgbus=kernel.msgbus,
            cache=kernel.cache,
            clock=kernel.clock,
            adapter=adapter,
            safety_rails=rails,
            instrument_provider=instrument_provider,
            on_safety_violation=self._on_safety_violation,
            is_run_gated=self._run_gate_provider,
        )
        kernel.exec_engine.register_client(client)

        # Step 8: bar 供給用 data client を **kernel.start() の前** に登録する。戦略の
        # on_start が `subscribe_bars(<...-INTERNAL>)` を呼ぶと engine は当該 venue 宛に
        # SubscribeTradeTicks を発行するため、その時点で client が登録済みでないと
        # aggregator が作られず on_bar が永遠に来ない。
        data_client = NautilusVenueDataClient(
            loop=loop,
            venue=Venue(venue_str),
            msgbus=kernel.msgbus,
            cache=kernel.cache,
            clock=kernel.clock,
            instrument_provider=instrument_provider,
        )
        kernel.data_engine.register_client(data_client)

        # 戦略インスタンス化（engine_runner の backtest と同じ contract）。
        # config= 形式の戦略は scenario/params から組めないため、kwargs 形式
        # （instrument_id / bar_type_str）を default として渡す（mean_reversion_01 等）。
        # 起動対象は **request の instrument_id**（kernel cache / RiskEngine に登録したのと
        # 同じ銘柄）。scenario の既定銘柄ではなくユーザーが指定した銘柄に従う。bar_type は
        # Live 用 INTERNAL（ADR-B: live aggregation 由来。EXTERNAL→INTERNAL 読み替えは
        # bar_supply に集約）。
        kwargs = {
            "instrument_id": instrument_id,
            "bar_type_str": live_bar_type(
                instrument_id, normalize_granularity(scenario["granularity"])
            ),
        }
        kwargs.update(params)
        strategy = strategy_cls(**kwargs)
        # Step 7 B: 発注主体 StrategyId を run の nautilus_strategy_id ("LIVE-{run}") に強制する。
        # Strategy.__init__ は self.id を `{ClassName}-{order_id_tag}` で採番するため
        # （StrategyConfig.strategy_id だけでは "-{tag}" が付いて一致しない）、
        # change_id で直接差し替える。**add_strategy の前**に行うことが必須:
        # register() が `events.order.{self.id}` を購読し order_factory を self.id で
        # 構成するため、ここで id を確定しないと order が旧 id を運ぶ。
        #
        # ⚠️ Trader.add_strategy は `order_id_tag in (None, "None")` のとき id を
        # `{id.partition('-')[0]}-{NNN}` に**再採番する**（"LIVE-ab12cd34" → "LIVE-000"）。
        # これを防ぐため order_id_tag も明示設定する（"LIVE-" の後ろ = run 短縮 id）。
        # 単一 run なので tag 衝突は起きない。Replay（engine_runner）は change_id を
        # 呼ばないので従来通り ClassName 由来のまま（id 強制は Live 経路に閉じる）。
        strategy.change_id(StrategyId(nautilus_strategy_id))
        strategy.change_order_id_tag(nautilus_strategy_id.partition("-")[2] or nautilus_strategy_id)
        kernel.trader.add_strategy(strategy)

        # Step 7 C: kernel msgbus の order events を購読し UI へ橋渡しする。
        # 戦略は `events.order.{strategy_id}` に publish するので、当該 run の id で購読する
        # （change_id 後 = nautilus_strategy_id）。handler は live loop thread で呼ばれる。
        if self._on_order_event is not None or self._on_telemetry is not None:
            kernel.msgbus.subscribe(
                topic=f"events.order.{nautilus_strategy_id}",
                handler=self._make_order_event_handler(
                    kernel, nautilus_strategy_id, venue_str
                ),
            )

        # §570 (Step 9 remediation): strategy が emit_strategy_log() で publish した UI ログ行
        # を購読して橋渡しする。order-event bridge と同じく当該 run の id で購読する。
        if self._on_strategy_log is not None:
            kernel.msgbus.subscribe(
                topic=strategy_log_topic(nautilus_strategy_id),
                handler=self._make_strategy_log_handler(nautilus_strategy_id),
            )

        kernel.start()

        self._kernel = kernel
        self._strategy = strategy
        self._strategy_id_str = str(strategy.id)
        self._data_client = data_client

        # Step 8: live tick を data client に流す。LiveRunner（共有 session の adapter→tick
        # pipeline）に listener を登録し、当該銘柄を購読させる（idempotent）。runner が無い
        # 構成（テストの直結 kernel）では tap を張らず、テストが data_client を直接叩く。
        runner = self._runner_provider() if self._runner_provider is not None else None
        if runner is not None:
            try:
                for live_instrument_id in live_instrument_ids:
                    await runner.subscribe(live_instrument_id)
            except Exception:  # noqa: BLE001 — 既購読/購読失敗でも attach は続行（既存 UI 購読を尊重）
                log.exception("runner.subscribe failed during attach")
            self._runner = runner
            self._tick_listener = self._make_tick_listener(data_client, live_instrument_ids)
            runner.add_tick_listener(self._tick_listener)

    @staticmethod
    def _live_instrument_ids(*, primary_instrument_id: str, scenario: dict) -> list[str]:
        """Return the live universe to register before strategy on_start subscriptions."""

        result: list[str] = []
        seen: set[str] = set()
        for candidate in [primary_instrument_id, *(scenario.get("instruments") or [])]:
            if not isinstance(candidate, str) or not candidate:
                continue
            if candidate in seen:
                continue
            seen.add(candidate)
            result.append(candidate)
        return result

    def _make_tick_listener(self, data_client, instrument_ids: list[str]):
        """LiveRunner 用の tick listener を作る (Step 8)。

        当該 run の instruments の `TradesUpdate` のみを data client に渡す。listener は
        live loop thread 上で同期呼び出しされる（`feed_trades_update` は `_handle_data` =
        msgbus.send のみで blocking しない、§Step4 不変条件と整合）。best-effort。
        """
        allowed = set(instrument_ids)

        def _listener(trade) -> None:
            try:
                if str(trade.instrument_id) not in allowed:
                    return
                data_client.feed_trades_update(trade)
            except Exception:  # noqa: BLE001 — bar 供給の失敗で戦略/pipeline を止めない
                log.exception("tick→TradeTick feed failed")

        return _listener

    def _make_strategy_log_handler(self, strategy_id: str):
        """`strategy.log.{strategy_id}` の msgbus handler を作る (§570 remediation)。

        strategy が `emit_strategy_log()` で publish した `StrategyLogRecord` を受け、
        `on_strategy_log(record, strategy_id)` で UI へ橋渡しする。

        ⚠️ live loop thread 上で呼ばれる。order-event bridge と同じく best-effort で、
        重い処理・blocking round-trip・外側 lock の取得はしない（§Step4 不変条件）。
        """

        def _handler(record) -> None:
            try:
                if self._on_strategy_log is not None:
                    self._on_strategy_log(record, strategy_id)
            except Exception:  # noqa: BLE001 — bridge は best-effort（戦略を止めない）
                log.exception("strategy-log bridge handler failed")

        return _handler

    def _make_order_event_handler(self, kernel, strategy_id: str, venue_str: str):
        """`events.order.{strategy_id}` の msgbus handler を作る (Step 7 C)。

        Nautilus OrderEvent を受け、`cache.order(client_order_id)` で order を引いて
        UI 互換の `OrderEventData` を構成し、`on_order_event(ev, strategy_id)` を呼ぶ。
        その後 `on_telemetry(strategy_id, metrics)` で run 別 telemetry を push する
        （fill / order の都度更新）。

        ⚠️ live loop thread 上で呼ばれる。重い処理・blocking round-trip・外側 lock の
        取得はしない（自己デッドロック回避、§Step4 不変条件）。

        重複報告の注意: 実 venue では同じ約定が共有 adapter の EC stream
        （`_backend_impl._publish_order_event`、strategy_id 空）でも届き得るが、UI 側は
        client_order_id でマージし「非空が勝つ」規則で LIVE-{run} を保持する。mock の
        EC stream は発火しないため Step 7 のテスト範囲では二重化しない。
        """

        def _handler(event) -> None:
            try:
                ev_data = self._order_event_data(kernel, event)
                if ev_data is not None and self._on_order_event is not None:
                    self._on_order_event(ev_data, strategy_id)
            except Exception:  # noqa: BLE001 — bridge は best-effort（戦略を止めない）
                log.exception("order-event bridge handler failed")
            try:
                if self._on_telemetry is not None:
                    self._on_telemetry(
                        strategy_id, self._compute_telemetry(kernel, venue_str)
                    )
            except Exception:  # noqa: BLE001
                log.exception("telemetry compute/push failed")

        return _handler

    @staticmethod
    def _order_event_data(kernel, event):
        """Nautilus OrderEvent → UI 互換 `OrderEventData`（Step 7 C）。

        order を cache から引けない（未登録/同期前）場合は None を返してスキップする。
        mock では venue_order_id が無いので client_order_id を order_id に流用する
        （ManualOrderFacade の正規化と同方針）。
        """
        from engine.live.order_types import OrderEventData

        client_order_id = getattr(event, "client_order_id", None)
        if client_order_id is None:
            return None
        order = kernel.cache.order(client_order_id)
        if order is None:
            return None
        venue_order_id = order.venue_order_id
        veid = venue_order_id.value if venue_order_id is not None else ""
        return OrderEventData(
            order_id=client_order_id.value,
            venue_order_id=veid,
            client_order_id=client_order_id.value,
            status=order.status.name,
            filled_qty=float(order.filled_qty),
            avg_price=float(order.avg_px),
            ts_ms=int(kernel.clock.timestamp_ns() // 1_000_000),
        )

    @staticmethod
    def _compute_telemetry(kernel, venue_str: str) -> dict:
        """run（= 単一 kernel、§0.7）の telemetry を算出する（Step 7 D）。

        - order_count = cache.orders() 件数（この kernel は単一 run なので全件 = この run）。
        - fill_count = filled_qty>0 の order 数（OrderFilled カウンタの代理。終端/部分約定を
          問わず「約定が付いた注文」を数える。単純で cache だけで再現でき、再起動時の
          ドリフトが無い）。
        - realized_pnl / unrealized_pnl = Portfolio の public API を JPY で合算。市場データ
          未供給（Step 8 前）では unrealized は空 dict（→ 0.0）になり得るのを graceful に扱う。

        live loop thread から呼ばれるため lock を取らず軽量に保つ。
        """
        from nautilus_trader.model.currencies import JPY
        from nautilus_trader.model.identifiers import Venue

        orders = kernel.cache.orders()
        order_count = len(orders)
        fill_count = sum(1 for o in orders if float(o.filled_qty) > 0.0)

        venue = Venue(venue_str)
        realized = NautilusLiveEngineController._sum_jpy(
            kernel.portfolio.realized_pnls(venue, target_currency=JPY)
        )
        unrealized = NautilusLiveEngineController._sum_jpy(
            kernel.portfolio.unrealized_pnls(venue, target_currency=JPY)
        )
        return {
            "realized_pnl": realized,
            "unrealized_pnl": unrealized,
            "order_count": order_count,
            "fill_count": fill_count,
        }

    @staticmethod
    def _sum_jpy(pnls: dict) -> float:
        """`Portfolio.*_pnls()` の dict[Currency, Money] から JPY 額を取り出す。

        target_currency=JPY 指定時は JPY のみ含まれる。空 dict（建玉なし / market data
        未供給 / 換算失敗）は 0.0。`Money.as_double()` で JPY 額を取る。
        """
        from nautilus_trader.model.currencies import JPY

        total = 0.0
        for currency, money in pnls.items():
            if currency == JPY:
                total += money.as_double()
        return total

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

        async def _cancel_and_wait() -> None:
            try:
                # 当該戦略の order **のみ** cancel（§1.3 / M6）。手動・他戦略は巻き込まない。
                # instrument 属性に依存せず cache を strategy.id で引く（戦略ごとに
                # instrument_id の保持名/型が異なるため）。teardown 中は accepted（open）に
                # 加え submit 済み未 ack（inflight）も取り消す——どちらも放置すると venue に
                # 残る。両 index は排他なので二重 cancel しない。
                inflight = kernel.cache.orders_inflight(strategy_id=strategy.id)
                for order in list(kernel.cache.orders_open(strategy_id=strategy.id)) + inflight:
                    strategy.cancel_order(order)
            except Exception:  # noqa: BLE001 — best-effort
                log.exception("cancel_inflight_orders failed")
                return

            # cancel コマンドをキューに積んだだけでは exec client はまだ HTTP DELETE を送っていない。
            # generate_order_pending_cancel が呼ばれると order は PENDING_CANCEL に遷移し
            # orders_open / orders_inflight の両インデックスから外れる。それを確認してから
            # 呼び出し元が kernel.stop() に進めるよう、ここでポーリングして待つ（issue #15）。
            import asyncio as _aio

            deadline = _aio.get_running_loop().time() + 3.0
            while True:
                remaining = list(kernel.cache.orders_open(strategy_id=strategy.id)) + list(
                    kernel.cache.orders_inflight(strategy_id=strategy.id)
                )
                if not remaining:
                    break
                if _aio.get_running_loop().time() >= deadline:
                    log.warning(
                        "cancel_inflight_orders: %d order(s) still open/inflight after 3 s; "
                        "proceeding with kernel stop",
                        len(remaining),
                    )
                    break
                await _aio.sleep(0.05)

        try:
            asyncio.run_coroutine_threadsafe(_cancel_and_wait(), loop).result(timeout=6.0)
        except Exception:  # noqa: BLE001
            log.exception("cancel_inflight_orders scheduling failed")

    def _teardown_kernel(self) -> None:
        # Step 8: live tick tap を外す（kernel teardown の前に。listener が破棄済み
        # data client / kernel を叩かないように）。runner が無ければ no-op。
        if self._runner is not None and self._tick_listener is not None:
            self._runner.remove_tick_listener(self._tick_listener)
        self._tick_listener = None
        self._runner = None
        self._data_client = None

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
            # kernel.start()（sync）で起動したため、stop_async() との非対称を避けて
            # 同期 kernel.stop() で対称化する。stop() は _await_engines_disconnected()
            # を待たないのでハングしない（issue #15）。
            async def _stop() -> None:
                kernel.stop()

            asyncio.run_coroutine_threadsafe(_stop(), loop).result(timeout=10.0)
        except Exception:  # noqa: BLE001 — 停止失敗でも run state は terminal にする
            log.exception("kernel stop failed during detach")
