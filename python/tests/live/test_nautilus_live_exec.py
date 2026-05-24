"""Integration: NautilusVenueExecClient + safety rails over the mock adapter (Step 4).

Drives real Nautilus orders (a one-shot LIMIT strategy) through a real live stack
(Kernel + Trader + LiveRiskEngine + LiveExecutionEngine + custom exec client) into the
MockVenueAdapter, verifying:
- within-limit order reaches the adapter and fills (FILLED in cache);
- native LiveRiskEngineConfig.max_notional_per_order denies an oversized order before the
  adapter is touched (OrderDenied);
- custom pre-trade rails (max_position_size_jpy / allowed_instruments) deny in the exec
  client and emit a SafetyRailViolation, without touching the adapter.

These are the Step 4 "structural safety" success criteria (§5 Safety Rails). Bar/market-data
supply to the strategy is Step 8, so we submit explicitly in on_start (no bars needed).
"""

import asyncio

import pytest
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.config import LiveDataEngineConfig, LiveExecEngineConfig
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId, Venue
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.system.kernel import NautilusKernel
from nautilus_trader.trading.strategy import Strategy

from engine.live.mock_adapter import MockVenueAdapter
from engine.live.nautilus_exec_client import NautilusVenueExecClient
from engine.live.safety_rails import SafetyLimits, SafetyRails
from engine.strategy_runtime.instrument_factory import make_equity_instrument

_IID = "7203.TSE"


class _SpyAdapter(MockVenueAdapter):
    """submit_order の呼び出しを記録する mock（rail が venue に届かないことの検証用）。"""

    def __init__(self) -> None:
        super().__init__()
        self.submit_calls: list[dict] = []

    async def submit_order(self, **kwargs):
        self.submit_calls.append(kwargs)
        return await super().submit_order(**kwargs)


class _OneShotLimit(Strategy):
    """on_start で 1 回だけ LIMIT BUY を出す最小戦略（テスト用）。"""

    def __init__(self, instrument_id: str, qty: int, price: float) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._qty = qty
        self._price = price

    def on_start(self) -> None:
        order = self.order_factory.limit(
            self._iid,
            OrderSide.BUY,
            Quantity.from_int(self._qty),
            Price(self._price, precision=1),
        )
        self.submit_order(order)


class _Harness:
    """Kernel + custom exec client + one-shot strategy を組んで回す薄いテストハーネス。"""

    def __init__(self, *, rails: SafetyRails, adapter: MockVenueAdapter, is_run_gated=None) -> None:
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        self.violations = []
        cfg = TradingNodeConfig(
            trader_id="LIVEHOST-001",
            logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
            exec_engine=LiveExecEngineConfig(),
            risk_engine=rails.to_live_risk_engine_config([_IID]),
            data_engine=LiveDataEngineConfig(),
        )
        self.kernel = NautilusKernel(name="LiveHost", config=cfg, loop=self.loop)
        self.kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
        self.adapter = adapter
        self.client = NautilusVenueExecClient(
            loop=self.loop,
            venue=Venue("TSE"),
            msgbus=self.kernel.msgbus,
            cache=self.kernel.cache,
            clock=self.kernel.clock,
            adapter=adapter,
            safety_rails=rails,
            instrument_provider=InstrumentProvider(),
            on_safety_violation=self.violations.append,
            is_run_gated=is_run_gated,
        )
        self.kernel.exec_engine.register_client(self.client)

    def run_strategy(self, qty: int, price: float, settle: float = 0.3) -> None:
        self.kernel.trader.add_strategy(_OneShotLimit(_IID, qty, price))

        async def _run():
            self.kernel.start()
            await asyncio.sleep(settle)
            await self.kernel.stop_async()

        self.loop.run_until_complete(_run())

    def order_statuses(self) -> list[str]:
        return [o.status.name for o in self.kernel.cache.orders()]

    def close(self) -> None:
        if not self.loop.is_closed():
            self.loop.close()


@pytest.fixture
def logged_in_adapter():
    adapter = _SpyAdapter()
    adapter.is_logged_in = True
    adapter.set_account_snapshot(cash=10_000_000.0, buying_power=10_000_000.0)
    return adapter


def test_within_limits_order_reaches_adapter_and_fills(logged_in_adapter):
    logged_in_adapter.set_next_order_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=500_000, max_position_size_jpy=1_000_000))
    h = _Harness(rails=rails, adapter=logged_in_adapter)
    try:
        # 100 株 * 2500 = 250,000 JPY < 500k cap
        h.run_strategy(qty=100, price=2500.0)
        assert logged_in_adapter.submit_calls, "adapter.submit_order must be called"
        assert "FILLED" in h.order_statuses()
        assert h.violations == []
    finally:
        h.close()


def test_native_max_notional_denies_before_adapter(logged_in_adapter):
    # cap 100k, order 100*2500=250k → RiskEngine denies natively
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=100_000))
    h = _Harness(rails=rails, adapter=logged_in_adapter)
    try:
        h.run_strategy(qty=100, price=2500.0)
        assert "DENIED" in h.order_statuses()
        assert not logged_in_adapter.submit_calls, "denied order must not reach the venue"
    finally:
        h.close()


def test_custom_position_size_denies_and_emits_violation(logged_in_adapter):
    # position cap 100k, order notional 250k → custom pre-trade rail denies in the client
    rails = SafetyRails(SafetyLimits(max_position_size_jpy=100_000))
    h = _Harness(rails=rails, adapter=logged_in_adapter)
    try:
        h.run_strategy(qty=100, price=2500.0)
        assert "DENIED" in h.order_statuses()
        assert not logged_in_adapter.submit_calls
        assert any(v.kind == "MAX_POSITION_SIZE" for v in h.violations)
    finally:
        h.close()


def test_allowed_instruments_whitelist_denies(logged_in_adapter):
    rails = SafetyRails(SafetyLimits(allowed_instruments=("9984.TSE",)))  # 7203 not allowed
    h = _Harness(rails=rails, adapter=logged_in_adapter)
    try:
        h.run_strategy(qty=100, price=2500.0)
        assert "DENIED" in h.order_statuses()
        assert not logged_in_adapter.submit_calls
        assert any(v.kind == "ALLOWED_INSTRUMENTS" for v in h.violations)
    finally:
        h.close()


# --- Issue #6: PAUSE gates new orders (is_run_gated seam) --------------------


def test_paused_run_denies_new_order_before_venue(logged_in_adapter):
    """Issue #6: PAUSED の run（is_run_gated→True）からの submit は venue に届かず
    `OrderDenied`（DENIED）になる。state_machine.is_running が False の間 host/exec client が
    新規発注を deny する、という docstring の主張を実装で担保する。"""
    logged_in_adapter.set_next_order_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    # 数量/価格は十分小さく、rails では弾かれない（gate のみが deny 理由になることを保証）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))
    h = _Harness(rails=rails, adapter=logged_in_adapter, is_run_gated=lambda sid: True)
    try:
        h.run_strategy(qty=100, price=2500.0)
        assert "DENIED" in h.order_statuses(), "paused run order must be DENIED"
        assert not logged_in_adapter.submit_calls, "paused order must not reach the venue"
        # PAUSE gate は safety-rail 違反ではない（on_safety_violation は発火しない）。
        assert h.violations == []
    finally:
        h.close()


def test_running_run_submits_when_not_gated(logged_in_adapter):
    """Issue #6 リグレッション防止: gate が開いている（is_run_gated→False）RUNNING の run は
    従来どおり venue に届いて約定する。PAUSE gate が常時 deny になっていないことを確認する。"""
    logged_in_adapter.set_next_order_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))
    h = _Harness(rails=rails, adapter=logged_in_adapter, is_run_gated=lambda sid: False)
    try:
        h.run_strategy(qty=100, price=2500.0)
        assert logged_in_adapter.submit_calls, "non-gated order must reach the venue"
        assert "FILLED" in h.order_statuses()
        assert "DENIED" not in h.order_statuses()
    finally:
        h.close()


def test_no_gate_provider_submits_as_before(logged_in_adapter):
    """is_run_gated 未注入（None）なら従来挙動（手動発注経路など gate 概念が無い文脈）。"""
    logged_in_adapter.set_next_order_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))
    h = _Harness(rails=rails, adapter=logged_in_adapter, is_run_gated=None)
    try:
        h.run_strategy(qty=100, price=2500.0)
        assert logged_in_adapter.submit_calls
        assert "FILLED" in h.order_statuses()
    finally:
        h.close()


# --- NautilusLiveEngineController lifecycle (attach builds kernel, detach tears down) ---


class _KwargsStrat(Strategy):
    """kwargs 形式（instrument_id / bar_type_str）の最小戦略（controller の attach 検証用）。"""

    def __init__(self, instrument_id: str, bar_type_str: str) -> None:
        super().__init__()
        self._iid = instrument_id
        self._bar_type_str = bar_type_str


class _OpenLimitKwargsStrat(Strategy):
    """kwargs 形式。on_start で resting LIMIT BUY を 1 回出す。`instrument_id` を public
    属性に持たない（`self._iid`）ことで、cancel が `strategy.id` 経由で効くことを検証する。"""

    def __init__(self, instrument_id: str, bar_type_str: str) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._bar_type_str = bar_type_str

    def on_start(self) -> None:
        order = self.order_factory.limit(
            self._iid, OrderSide.BUY, Quantity.from_int(100), Price(2500.0, precision=1)
        )
        self.submit_order(order)


class _OneShotLimitKwargs(Strategy):
    """kwargs 形式（controller の attach 契約）。on_start で 1 回 LIMIT BUY を出す。

    `instrument_id` / `bar_type_str` を受ける（mean_reversion 系サンプル戦略と同形）。
    fill 結果は adapter の set_next_order_outcome が決める。"""

    def __init__(self, instrument_id: str, bar_type_str: str) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._bar_type_str = bar_type_str

    def on_start(self) -> None:
        order = self.order_factory.limit(
            self._iid, OrderSide.BUY, Quantity.from_int(100), Price(2500.0, precision=1)
        )
        self.submit_order(order)


class _LogEmitter(Strategy):
    """on_start で UI ログ行を 1 本 emit する最小戦略（§570 strategy-log bridge 検証用）。

    attach の kwargs 契約（instrument_id / bar_type_str）に合わせる。発注も購読もしない
    ので data client は不要——bridge が emit を拾えることだけを見る。"""

    def __init__(self, instrument_id: str, bar_type_str: str) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)

    def on_start(self) -> None:
        from engine.live.strategy_log import emit_strategy_log

        emit_strategy_log(self, "strategy started", "INFO")


def _bg_loop():
    import threading

    loop = asyncio.new_event_loop()
    t = threading.Thread(target=loop.run_forever, daemon=True)
    t.start()
    return loop, t


def _stop_bg_loop(loop, t) -> None:
    loop.call_soon_threadsafe(loop.stop)
    t.join(timeout=5)
    if not loop.is_closed():
        loop.close()


def test_cancel_inflight_orders_cancels_by_strategy_id(logged_in_adapter):
    """Finding 2 (Step 4 review): cancel は cache を `strategy.id` で引く。戦略に
    `instrument_id` 属性が無くても、当該 run の open 注文を確実に cancel する。"""
    import time as _time

    from engine.live.engine_controller import NautilusLiveEngineController

    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop, adapter_provider=lambda: logged_in_adapter
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}

    async def _collect():
        strat = controller._strategy
        return [
            o.status.name
            for o in controller._kernel.cache.orders_open(strategy_id=strat.id)
        ]

    def _open():
        return asyncio.run_coroutine_threadsafe(_collect(), loop).result(timeout=5)

    def _wait(predicate) -> None:
        deadline = _time.time() + 5
        while _time.time() < deadline and not predicate():
            _time.sleep(0.05)

    try:
        controller.attach(
            strategy_cls=_OpenLimitKwargsStrat,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-cncl0001",
            session=None,
            safety_rails=SafetyRails(SafetyLimits()),
        )
        # 旧実装が頼っていた public 属性が無い（= 旧コードなら cancel が no-op になる）。
        assert not hasattr(controller._strategy, "instrument_id")

        _wait(_open)
        assert _open(), "LIMIT order should rest open before cancel"

        controller.cancel_inflight_orders(nautilus_strategy_id="LIVE-cncl0001")
        _wait(lambda: not _open())
        assert not _open(), "strategy's open orders must be cancelled by strategy_id"
    finally:
        controller.detach(nautilus_strategy_id="LIVE-cncl0001")
        _stop_bg_loop(loop, t)


def test_attach_uses_request_instrument_not_scenario(logged_in_adapter):
    """Finding 3 (Step 4 review): 戦略 kwargs は **request の instrument_id**（kernel cache に
    登録した銘柄）を使い、scenario の既定銘柄は使わない。bar_type は Live INTERNAL。"""
    from engine.live.engine_controller import NautilusLiveEngineController

    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop, adapter_provider=lambda: logged_in_adapter
    )
    # scenario の既定は 9984.TSE。だが起動指定は 7203.TSE（_IID）。
    scenario = {"instruments": ["9984.TSE"], "granularity": "Minute"}
    try:
        controller.attach(
            strategy_cls=_KwargsStrat,
            scenario=scenario,
            instrument_id=_IID,  # 7203.TSE
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-iid00001",
            session=None,
            safety_rails=SafetyRails(SafetyLimits()),
        )
        strat = controller._strategy
        assert strat._iid == _IID  # scenario の 9984.TSE ではない
        assert strat._bar_type_str.startswith("7203.TSE")
        assert strat._bar_type_str.endswith("-INTERNAL")
    finally:
        controller.detach(nautilus_strategy_id="LIVE-iid00001")
        _stop_bg_loop(loop, t)


def test_controller_attach_then_detach_lifecycle(logged_in_adapter):
    """NautilusLiveEngineController が背景 loop 上で kernel を組み、detach で停止する。"""
    from engine.live.engine_controller import NautilusLiveEngineController

    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop,
        adapter_provider=lambda: logged_in_adapter,
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}
    try:
        controller.attach(
            strategy_cls=_KwargsStrat,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-test1234",
            session=None,
            safety_rails=SafetyRails(SafetyLimits(max_order_value_jpy=500_000)),
        )
        assert controller._kernel is not None
        assert controller._strategy is not None

        controller.detach(nautilus_strategy_id="LIVE-test1234")
        assert controller._kernel is None
    finally:
        _stop_bg_loop(loop, t)


# --- Step 7 B: StrategyId is forced to nautilus_strategy_id ------------------


def test_attach_forces_strategy_id_to_nautilus_strategy_id(logged_in_adapter):
    """Step 7 B: attach 後 `str(strategy.id) == nautilus_strategy_id`（"LIVE-...")。

    既定の Nautilus 採番（ClassName-None）ではなく run の StrategyId に揃え、RunRegistry の
    逆引きと整合させる。これにより cancel_inflight_orders が同じ id で order を引ける。"""
    from engine.live.engine_controller import NautilusLiveEngineController

    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop, adapter_provider=lambda: logged_in_adapter
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}
    try:
        controller.attach(
            strategy_cls=_KwargsStrat,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-forced01",
            session=None,
            safety_rails=SafetyRails(SafetyLimits()),
        )
        assert str(controller._strategy.id) == "LIVE-forced01"
    finally:
        controller.detach(nautilus_strategy_id="LIVE-forced01")
        _stop_bg_loop(loop, t)


def test_orders_indexable_by_forced_strategy_id(logged_in_adapter):
    """Step 7 B 回帰: 注文を出した戦略の order が forced strategy.id で cache から引ける。"""
    from engine.live.engine_controller import NautilusLiveEngineController

    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop, adapter_provider=lambda: logged_in_adapter
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}

    def _orders_for_id():
        async def _collect():
            strat = controller._strategy
            return [o.client_order_id.value for o in controller._kernel.cache.orders(strategy_id=strat.id)]

        return asyncio.run_coroutine_threadsafe(_collect(), loop).result(timeout=5)

    import time as _time

    try:
        controller.attach(
            strategy_cls=_OpenLimitKwargsStrat,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-idx00001",
            session=None,
            safety_rails=SafetyRails(SafetyLimits()),
        )
        deadline = _time.time() + 5
        while _time.time() < deadline and not _orders_for_id():
            _time.sleep(0.05)
        assert _orders_for_id(), "order must be indexable by the forced strategy.id"
    finally:
        controller.detach(nautilus_strategy_id="LIVE-idx00001")
        _stop_bg_loop(loop, t)


# --- Step 7 C/D: kernel msgbus order-event bridge + telemetry ----------------


def test_order_event_bridge_emits_with_strategy_id(logged_in_adapter):
    """Step 7 C: kernel 内の order events が on_order_event callback に LIVE-... 付きで届く。"""
    from engine.live.engine_controller import NautilusLiveEngineController

    logged_in_adapter.set_next_order_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    loop, t = _bg_loop()
    events: list = []
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop,
        adapter_provider=lambda: logged_in_adapter,
        on_order_event=lambda ev, strategy_id: events.append((ev, strategy_id)),
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}

    import time as _time

    try:
        controller.attach(
            strategy_cls=_OneShotLimitKwargs,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-bridge01",
            session=None,
            safety_rails=SafetyRails(SafetyLimits(max_order_value_jpy=1_000_000)),
        )
        deadline = _time.time() + 5
        while _time.time() < deadline and not any(
            ev.status == "FILLED" for ev, _ in events
        ):
            _time.sleep(0.05)
        # FILLED イベントが LIVE-bridge01 付きで届く。
        assert any(sid == "LIVE-bridge01" and ev.status == "FILLED" for ev, sid in events)
        filled = next(ev for ev, _ in events if ev.status == "FILLED")
        assert filled.filled_qty == 100
        assert filled.avg_price == 2500.0
        assert filled.client_order_id  # non-empty
    finally:
        controller.detach(nautilus_strategy_id="LIVE-bridge01")
        _stop_bg_loop(loop, t)


def test_strategy_log_bridge_emits_with_strategy_id(logged_in_adapter):
    """§570 remediation: emit_strategy_log() の行が on_strategy_log callback に
    LIVE-... 付き StrategyLogRecord で届く（kernel msgbus 経由の bridge）。"""
    import time as _time

    from engine.live.engine_controller import NautilusLiveEngineController

    loop, t = _bg_loop()
    logs: list = []
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop,
        adapter_provider=lambda: logged_in_adapter,
        on_strategy_log=lambda rec, sid: logs.append((rec, sid)),
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}

    try:
        controller.attach(
            strategy_cls=_LogEmitter,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-logbr01",
            session=None,
            safety_rails=SafetyRails(SafetyLimits(max_order_value_jpy=1_000_000)),
        )
        deadline = _time.time() + 5
        while _time.time() < deadline and not logs:
            _time.sleep(0.05)
        assert logs, "strategy-log bridge must fire on emit_strategy_log"
        rec, sid = logs[-1]
        assert sid == "LIVE-logbr01"
        assert rec.level == "INFO"
        assert rec.message == "strategy started"
        assert rec.ts_ns > 0
    finally:
        controller.detach(nautilus_strategy_id="LIVE-logbr01")
        _stop_bg_loop(loop, t)


def test_telemetry_callback_after_fill(logged_in_adapter):
    """Step 7 D: order event 受信時に telemetry callback が妥当な値で呼ばれる。"""
    from engine.live.engine_controller import NautilusLiveEngineController

    logged_in_adapter.set_next_order_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    loop, t = _bg_loop()
    telem: list = []
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop,
        adapter_provider=lambda: logged_in_adapter,
        on_telemetry=lambda strategy_id, m: telem.append((strategy_id, m)),
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}

    import time as _time

    try:
        controller.attach(
            strategy_cls=_OneShotLimitKwargs,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-telem001",
            session=None,
            safety_rails=SafetyRails(SafetyLimits(max_order_value_jpy=1_000_000)),
        )
        deadline = _time.time() + 5
        while _time.time() < deadline and not any(
            m["fill_count"] >= 1 for _, m in telem
        ):
            _time.sleep(0.05)
        assert telem, "telemetry callback must fire on order events"
        sid, last = telem[-1]
        assert sid == "LIVE-telem001"
        assert last["order_count"] >= 1
        assert last["fill_count"] >= 1
        # realized/unrealized は market data 未供給で 0 になり得る（graceful）。数値であること。
        assert isinstance(last["realized_pnl"], float)
        assert isinstance(last["unrealized_pnl"], float)
    finally:
        controller.detach(nautilus_strategy_id="LIVE-telem001")
        _stop_bg_loop(loop, t)


# --- Issue #12: modify が CANCELED を返したら order は CANCELED に遷移する ---


class _SubmitThenModify(Strategy):
    """on_start で resting LIMIT BUY を 1 回出し、ACCEPTED になったら一度だけ modify する
    最小戦略（Issue #12 検証用）。modify には quantity を明示指定し、別件の quantity=None
    バグ（後続サイクルで扱う）を切り離して CANCELED 遷移だけを観測する。"""

    def __init__(self, instrument_id: str, qty: int, price: float) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._qty = qty
        self._price = price
        self._order = None
        self._modified = False

    def on_start(self) -> None:
        order = self.order_factory.limit(
            self._iid,
            OrderSide.BUY,
            Quantity.from_int(self._qty),
            Price(self._price, precision=1),
        )
        self._order = order
        self.submit_order(order)

    def on_event(self, event) -> None:
        if self._modified or self._order is None:
            return
        o = self.cache.order(self._order.client_order_id)
        if o is not None and o.status.name == "ACCEPTED":
            self._modified = True
            self.modify_order(
                o,
                quantity=Quantity.from_int(self._qty),
                price=Price(self._price + 1.0, precision=1),
            )


def test_modify_returning_canceled_transitions_order_to_canceled(logged_in_adapter):
    """Issue #12: modify が終端 status CANCELED を返したら、cache の order は CANCELED に
    遷移しなければならない。現状は generate_order_updated で ACCEPTED のまま固着し、
    「取消済みのはずが Nautilus 上 live」という危険な乖離になる。"""
    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    logged_in_adapter.set_next_modify_outcome(status="CANCELED")
    # rails では弾かれない十分大きな cap（gate/rail ではなく modify 経路だけを観測）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    cfg = TradingNodeConfig(
        trader_id="LIVEHOST-001",
        logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
        exec_engine=LiveExecEngineConfig(),
        risk_engine=rails.to_live_risk_engine_config([_IID]),
        data_engine=LiveDataEngineConfig(),
    )
    kernel = NautilusKernel(name="LiveHost", config=cfg, loop=loop)
    kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
    client = NautilusVenueExecClient(
        loop=loop,
        venue=Venue("TSE"),
        msgbus=kernel.msgbus,
        cache=kernel.cache,
        clock=kernel.clock,
        adapter=logged_in_adapter,
        safety_rails=rails,
        instrument_provider=InstrumentProvider(),
    )
    kernel.exec_engine.register_client(client)
    kernel.trader.add_strategy(_SubmitThenModify(_IID, 100, 2500.0))

    async def _run():
        kernel.start()
        await asyncio.sleep(0.5)
        await kernel.stop_async()

    try:
        loop.run_until_complete(_run())
        statuses = [o.status.name for o in kernel.cache.orders()]
        # observable behavior: CANCELED を返した modify の後、order は CANCELED であること。
        assert "CANCELED" in statuses, f"order must be CANCELED after modify→CANCELED, got {statuses}"
        # 固着していないこと（ACCEPTED のまま live でない）。
        assert "ACCEPTED" not in statuses, f"order must not stay ACCEPTED (live) after cancel, got {statuses}"
    finally:
        if not loop.is_closed():
            loop.close()


def test_modify_returning_filled_transitions_order_to_filled(logged_in_adapter):
    """Issue #12: modify が終端 status FILLED を返したら、cache の order は FILLED に
    遷移しなければならない。現状は「それ以外」分岐で generate_order_updated になり ACCEPTED
    のまま固着する（約定済みのはずが Nautilus 上 live）。"""
    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    logged_in_adapter.set_next_modify_outcome(status="FILLED", filled_qty=100, avg_price=2500.0)
    # rails では弾かれない十分大きな cap（gate/rail ではなく modify 経路だけを観測）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    cfg = TradingNodeConfig(
        trader_id="LIVEHOST-001",
        logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
        exec_engine=LiveExecEngineConfig(),
        risk_engine=rails.to_live_risk_engine_config([_IID]),
        data_engine=LiveDataEngineConfig(),
    )
    kernel = NautilusKernel(name="LiveHost", config=cfg, loop=loop)
    kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
    client = NautilusVenueExecClient(
        loop=loop,
        venue=Venue("TSE"),
        msgbus=kernel.msgbus,
        cache=kernel.cache,
        clock=kernel.clock,
        adapter=logged_in_adapter,
        safety_rails=rails,
        instrument_provider=InstrumentProvider(),
    )
    kernel.exec_engine.register_client(client)
    kernel.trader.add_strategy(_SubmitThenModify(_IID, 100, 2500.0))

    async def _run():
        kernel.start()
        await asyncio.sleep(0.5)
        await kernel.stop_async()

    try:
        loop.run_until_complete(_run())
        statuses = [o.status.name for o in kernel.cache.orders()]
        # observable behavior: FILLED を返した modify の後、order は FILLED であること。
        assert "FILLED" in statuses, f"order must be FILLED after modify→FILLED, got {statuses}"
        # 固着していないこと（ACCEPTED のまま live でない）。
        assert "ACCEPTED" not in statuses, f"order must not stay ACCEPTED (live) after fill, got {statuses}"
    finally:
        if not loop.is_closed():
            loop.close()


# --- quantity=None modify (price-only): order must not stick at PENDING_UPDATE ---


class _SubmitThenModifyPriceOnly(Strategy):
    """on_start で resting LIMIT BUY を 1 回出し、ACCEPTED になったら一度だけ
    **price だけ**変更する modify を出す（quantity は渡さない）。

    Nautilus の Strategy.modify_order は quantity 省略時に order の現数量で埋めず
    ModifyOrder.quantity=None のまま exec client に届ける（_create_modify_order で
    quantity=quantity をそのまま使う）。よってこの戦略は quantity=None が
    _modify_order に届く経路を再現する（quantity=None バグ検証用）。"""

    def __init__(self, instrument_id: str, qty: int, price: float) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._qty = qty
        self._price = price
        self._order = None
        self._modified = False

    def on_start(self) -> None:
        order = self.order_factory.limit(
            self._iid,
            OrderSide.BUY,
            Quantity.from_int(self._qty),
            Price(self._price, precision=1),
        )
        self._order = order
        self.submit_order(order)

    def on_event(self, event) -> None:
        if self._modified or self._order is None:
            return
        o = self.cache.order(self._order.client_order_id)
        if o is not None and o.status.name == "ACCEPTED":
            self._modified = True
            # quantity は渡さない（price だけ変更）→ ModifyOrder.quantity is None
            self.modify_order(o, price=Price(self._price + 1.0, precision=1))


def test_modify_price_only_keeps_order_live_not_stuck_pending_update(logged_in_adapter):
    """quantity=None バグ: price だけ変更する modify（quantity 未指定）の後、order は
    更新が反映され live（ACCEPTED）を維持しなければならない。

    現状は _modify_order が終端でない status のフォールバックで command.quantity(=None)
    を generate_order_updated にそのまま渡し、Nautilus 側の型付き Quantity 要求に反して
    OrderUpdated が emit されず、order が PENDING_UPDATE で固着する。"""
    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    # modify は仕込まず素の ACCEPTED（バグは generate_order_updated の None 渡し側）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    cfg = TradingNodeConfig(
        trader_id="LIVEHOST-001",
        logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
        exec_engine=LiveExecEngineConfig(),
        risk_engine=rails.to_live_risk_engine_config([_IID]),
        data_engine=LiveDataEngineConfig(),
    )
    kernel = NautilusKernel(name="LiveHost", config=cfg, loop=loop)
    kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
    client = NautilusVenueExecClient(
        loop=loop,
        venue=Venue("TSE"),
        msgbus=kernel.msgbus,
        cache=kernel.cache,
        clock=kernel.clock,
        adapter=logged_in_adapter,
        safety_rails=rails,
        instrument_provider=InstrumentProvider(),
    )
    kernel.exec_engine.register_client(client)
    kernel.trader.add_strategy(_SubmitThenModifyPriceOnly(_IID, 100, 2500.0))

    async def _run():
        kernel.start()
        await asyncio.sleep(0.5)
        await kernel.stop_async()

    try:
        loop.run_until_complete(_run())
        orders = list(kernel.cache.orders())
        statuses = [o.status.name for o in orders]
        # observable behavior: price-only modify の後、PENDING_UPDATE で固着しないこと。
        assert "PENDING_UPDATE" not in statuses, (
            f"price-only modify must not leave order stuck at PENDING_UPDATE, got {statuses}"
        )
        # live を維持していること（終端化もしていない）。
        assert "ACCEPTED" in statuses, f"order must stay live (ACCEPTED) after price-only modify, got {statuses}"
        # 新価格が反映されていること（modify の意味）。
        assert any(o.has_price and float(o.price) == 2501.0 for o in orders), (
            f"order price must reflect the price-only modify (2501.0), got "
            f"{[float(o.price) for o in orders if o.has_price]}"
        )
    finally:
        if not loop.is_closed():
            loop.close()


def test_modify_returning_expired_transitions_order_to_expired(logged_in_adapter):
    """Issue #12: modify が終端 status EXPIRED を返したら、cache の order は EXPIRED に
    遷移しなければならない。generate_order_updated のままだと ACCEPTED 据え置きで固着し、
    「失効済みのはずが Nautilus 上 live」という危険な乖離になる（実装分岐ありの回帰証跡）。"""
    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    logged_in_adapter.set_next_modify_outcome(status="EXPIRED")
    # rails では弾かれない十分大きな cap（gate/rail ではなく modify 経路だけを観測）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    cfg = TradingNodeConfig(
        trader_id="LIVEHOST-001",
        logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
        exec_engine=LiveExecEngineConfig(),
        risk_engine=rails.to_live_risk_engine_config([_IID]),
        data_engine=LiveDataEngineConfig(),
    )
    kernel = NautilusKernel(name="LiveHost", config=cfg, loop=loop)
    kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
    client = NautilusVenueExecClient(
        loop=loop,
        venue=Venue("TSE"),
        msgbus=kernel.msgbus,
        cache=kernel.cache,
        clock=kernel.clock,
        adapter=logged_in_adapter,
        safety_rails=rails,
        instrument_provider=InstrumentProvider(),
    )
    kernel.exec_engine.register_client(client)
    kernel.trader.add_strategy(_SubmitThenModify(_IID, 100, 2500.0))

    async def _run():
        kernel.start()
        await asyncio.sleep(0.5)
        await kernel.stop_async()

    try:
        loop.run_until_complete(_run())
        statuses = [o.status.name for o in kernel.cache.orders()]
        # observable behavior: EXPIRED を返した modify の後、order は EXPIRED であること。
        assert "EXPIRED" in statuses, f"order must be EXPIRED after modify→EXPIRED, got {statuses}"
        # 固着していないこと（ACCEPTED のまま live でない）。
        assert "ACCEPTED" not in statuses, f"order must not stay ACCEPTED (live) after expire, got {statuses}"
    finally:
        if not loop.is_closed():
            loop.close()


def test_modify_returning_rejected_reverts_order_to_accepted(logged_in_adapter):
    """Issue #12 案A: modify が拒否系 status REJECTED を返したら、order は PENDING_UPDATE で
    固着せず modify 前状態(ACCEPTED)に戻り、新価格は適用されない（元の 2500.0 のまま）。

    現状は _modify_order が REJECTED で早期 return するだけで、PENDING_UPDATE のまま固着する
    （実機 ['PENDING_UPDATE']）。observable behavior を status と price の両方で観測する。"""
    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    logged_in_adapter.set_next_modify_outcome(status="REJECTED", reject_reason="too late")
    # rails では弾かれない十分大きな cap（gate/rail ではなく modify 経路だけを観測）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    cfg = TradingNodeConfig(
        trader_id="LIVEHOST-001",
        logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
        exec_engine=LiveExecEngineConfig(),
        risk_engine=rails.to_live_risk_engine_config([_IID]),
        data_engine=LiveDataEngineConfig(),
    )
    kernel = NautilusKernel(name="LiveHost", config=cfg, loop=loop)
    kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
    client = NautilusVenueExecClient(
        loop=loop,
        venue=Venue("TSE"),
        msgbus=kernel.msgbus,
        cache=kernel.cache,
        clock=kernel.clock,
        adapter=logged_in_adapter,
        safety_rails=rails,
        instrument_provider=InstrumentProvider(),
    )
    kernel.exec_engine.register_client(client)
    # 既存 _SubmitThenModify は price+1.0 で modify する → 2500.0 → 2501.0 を試みる。
    kernel.trader.add_strategy(_SubmitThenModify(_IID, 100, 2500.0))

    async def _run():
        kernel.start()
        await asyncio.sleep(0.5)
        await kernel.stop_async()

    try:
        loop.run_until_complete(_run())
        orders = list(kernel.cache.orders())
        statuses = [o.status.name for o in orders]
        # 固着していないこと（PENDING_UPDATE のまま放置されない）。
        assert "PENDING_UPDATE" not in statuses, (
            f"order must not stay stuck at PENDING_UPDATE after modify→REJECTED, got {statuses}"
        )
        # 拒否なので modify 前状態(ACCEPTED)に戻ること。
        assert "ACCEPTED" in statuses, (
            f"order must revert to ACCEPTED after modify→REJECTED, got {statuses}"
        )
        # 新価格 2501.0 は適用されず元の 2500.0 のままであること。
        assert any(o.has_price and float(o.price) == 2500.0 for o in orders), (
            f"rejected modify must not apply the new price; expected 2500.0, got "
            f"{[float(o.price) for o in orders if o.has_price]}"
        )
    finally:
        if not loop.is_closed():
            loop.close()


def test_modify_returning_denied_reverts_order_to_accepted_without_new_price(logged_in_adapter):
    """Issue #12 案A: modify が拒否系 status DENIED を返したら、order は modify 前状態(ACCEPTED)
    に戻り、新価格は適用されない（元の 2500.0 のまま）。

    現状は DENIED が「それ以外」分岐で generate_order_updated 化され、新価格 2501.0 が適用された
    ACCEPTED で live 据え置きになる。DENIED では ACCEPTED 自体は現状でも真なので、価格で判別する
    のが要点（現状 RED: float(order.price) == 2500.0 が 2501.0 になり fail）。"""
    logged_in_adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0)  # resting
    logged_in_adapter.set_next_modify_outcome(status="DENIED", reject_reason="risk")
    # rails では弾かれない十分大きな cap（gate/rail ではなく modify 経路だけを観測）。
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=10_000_000, max_position_size_jpy=10_000_000))

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    cfg = TradingNodeConfig(
        trader_id="LIVEHOST-001",
        logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
        exec_engine=LiveExecEngineConfig(),
        risk_engine=rails.to_live_risk_engine_config([_IID]),
        data_engine=LiveDataEngineConfig(),
    )
    kernel = NautilusKernel(name="LiveHost", config=cfg, loop=loop)
    kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
    client = NautilusVenueExecClient(
        loop=loop,
        venue=Venue("TSE"),
        msgbus=kernel.msgbus,
        cache=kernel.cache,
        clock=kernel.clock,
        adapter=logged_in_adapter,
        safety_rails=rails,
        instrument_provider=InstrumentProvider(),
    )
    kernel.exec_engine.register_client(client)
    # 既存 _SubmitThenModify は price+1.0 で modify する → 2500.0 → 2501.0 を試みる。
    kernel.trader.add_strategy(_SubmitThenModify(_IID, 100, 2500.0))

    async def _run():
        kernel.start()
        await asyncio.sleep(0.5)
        await kernel.stop_async()

    try:
        loop.run_until_complete(_run())
        orders = list(kernel.cache.orders())
        statuses = [o.status.name for o in orders]
        # 固着していないこと。
        assert "PENDING_UPDATE" not in statuses, (
            f"order must not stay stuck at PENDING_UPDATE after modify→DENIED, got {statuses}"
        )
        # 拒否なので modify 前状態(ACCEPTED)に戻ること。
        assert "ACCEPTED" in statuses, (
            f"order must revert to ACCEPTED after modify→DENIED, got {statuses}"
        )
        # 新価格 2501.0 は適用されず元の 2500.0 のままであること（DENIED 判別の要点）。
        assert any(o.has_price and float(o.price) == 2500.0 for o in orders), (
            f"denied modify must not apply the new price; expected 2500.0, got "
            f"{[float(o.price) for o in orders if o.has_price]}"
        )
    finally:
        if not loop.is_closed():
            loop.close()
