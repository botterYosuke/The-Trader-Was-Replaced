"""Phase 10 Step 8 — Live Bar 供給の本丸（data client → LiveDataEngine → on_bar）。

§2.3 / ADR-B: live venue の約定（`TradesUpdate`）を `NautilusVenueDataClient` が
Nautilus `TradeTick` 化し、`LiveDataEngine` の internal aggregation（INTERNAL `BarType`）が
確定 `Bar` を組んで戦略の `on_bar` に届ける。これにより Replay（catalog の EXTERNAL `Bar`）と
Live（aggregation 由来の INTERNAL `Bar`）が同じ `Bar` 型・同じ `BarSpecification` に揃う。

検証:
  1. `trades_update_to_trade_tick` の変換（price/size/aggressor/trade_id）。
  2. 実 kernel + data client + 戦略で、注入した tick 列から `on_bar` に Bar が届き、
     OHLCV が手計算と一致し、spec が EXTERNAL catalog BarType と揃う（source だけ INTERNAL）。
  3. `NautilusLiveEngineController` が attach 時に runner へ tick listener を張り、
     listener 経由の `TradesUpdate` が data client → engine に届く。detach で listener を外す。

時間バーは LiveClock タイマー駆動のため、テストは決定論かつ高速になるよう SECOND バーを
使う（aggregation ロジックは MINUTE と同一 `TimeBarAggregator`。MINUTE の OHLCV 正しさは
Step 1 の `test_live_bar_supply.py` が TestClock で別途ロック済み）。
"""

from __future__ import annotations

import asyncio
import threading
import time as _time

import pytest
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.config import (
    LiveDataEngineConfig,
    LiveExecEngineConfig,
    LiveRiskEngineConfig,
)
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import AggregationSource, AggressorSide
from nautilus_trader.model.identifiers import InstrumentId, Venue
from nautilus_trader.system.kernel import NautilusKernel
from nautilus_trader.trading.strategy import Strategy

from engine.live.adapter import TradesUpdate
from engine.live.bar_supply import trades_update_to_trade_tick
from engine.live.nautilus_data_client import NautilusVenueDataClient
from engine.strategy_runtime.instrument_factory import make_equity_instrument

_IID = "7203.TSE"
_SECOND_INTERNAL = "7203.TSE-1-SECOND-LAST-INTERNAL"


def _tu(ts_ns: int, price: float, size: float, side: str = "buy", iid: str = _IID) -> TradesUpdate:
    return TradesUpdate(
        kind="trades",
        instrument_id=iid,
        ts_ns=ts_ns,
        price=price,
        size=size,
        aggressor_side=side,
    )


# ---------------------------------------------------------------------------
# 1. 変換: TradesUpdate → TradeTick
# ---------------------------------------------------------------------------


def test_trades_update_to_trade_tick_maps_fields():
    instrument = make_equity_instrument("7203", "TSE")
    tu = _tu(1_700_000_000_000_000_000, 2500.0, 100.0, side="sell")

    tick = trades_update_to_trade_tick(tu, instrument, seq=3)

    assert tick.instrument_id == instrument.id
    assert float(tick.price) == 2500.0
    assert float(tick.size) == 100.0
    assert tick.aggressor_side == AggressorSide.SELLER
    assert tick.ts_event == 1_700_000_000_000_000_000
    assert tick.ts_init == 1_700_000_000_000_000_000
    # trade_id は {ts_ns}-{seq}（同 ns でも seq で一意）
    assert tick.trade_id.value == "1700000000000000000-3"


class _SidelessTrade:
    """aggressor_side を持たない venue 入力（kabu の CurrentPrice 由来など）の代理。"""

    instrument_id = _IID
    ts_ns = 1
    price = 10.0
    size = 1.0


def test_trades_update_to_trade_tick_missing_side_is_no_aggressor():
    instrument = make_equity_instrument("7203", "TSE")
    tick = trades_update_to_trade_tick(_SidelessTrade(), instrument)
    assert tick.aggressor_side == AggressorSide.NO_AGGRESSOR


# ---------------------------------------------------------------------------
# 2. full-path: data client → LiveDataEngine internal aggregation → on_bar
# ---------------------------------------------------------------------------


class _CapturingBars(Strategy):
    """INTERNAL bar を購読し、届いた Bar を記録するだけの戦略。"""

    def __init__(self, instrument_id: str, bar_type_str: str) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._bt = bar_type_str
        self.bars: list[Bar] = []

    def on_start(self) -> None:
        self.subscribe_bars(BarType.from_str(self._bt))

    def on_bar(self, bar: Bar) -> None:
        self.bars.append(bar)


class _DataHarness:
    """Kernel + NautilusVenueDataClient + capturing strategy の薄いハーネス。"""

    def __init__(self) -> None:
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        cfg = TradingNodeConfig(
            trader_id="LIVEHOST-001",
            logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", print_config=False),
            exec_engine=LiveExecEngineConfig(),
            risk_engine=LiveRiskEngineConfig(),
            data_engine=LiveDataEngineConfig(),
        )
        self.kernel = NautilusKernel(name="LiveHost", config=cfg, loop=self.loop)
        self.kernel.cache.add_instrument(make_equity_instrument("7203", "TSE"))
        self.client = NautilusVenueDataClient(
            loop=self.loop,
            venue=Venue("TSE"),
            msgbus=self.kernel.msgbus,
            cache=self.kernel.cache,
            clock=self.kernel.clock,
            instrument_provider=InstrumentProvider(),
        )
        self.kernel.data_engine.register_client(self.client)
        self.strategy = _CapturingBars(_IID, _SECOND_INTERNAL)
        self.kernel.trader.add_strategy(self.strategy)

    def run_feeding(self, ticks: list[tuple[float, float]], settle: float = 2.4) -> None:
        async def _run():
            self.kernel.start()
            now = self.kernel.clock.timestamp_ns()
            # 全 tick を連続注入（同一 SECOND バケットに入る）。
            for i, (price, size) in enumerate(ticks):
                self.client.feed_trades_update(_tu(now, price, size))
            await asyncio.sleep(settle)  # SECOND タイマーが発火して bar が確定するのを待つ
            await self.kernel.stop_async()

        self.loop.run_until_complete(_run())

    def close(self) -> None:
        if not self.loop.is_closed():
            self.loop.close()


@pytest.mark.slow
def test_injected_ticks_reach_on_bar_with_matching_ohlcv():
    """注入した tick 列から `on_bar` に Bar が届き、OHLCV が手計算と一致する。"""
    h = _DataHarness()
    try:
        # open=1000 / high=1010 / low=990 / close=1005 / volume=1000
        h.run_feeding([(1000.0, 100.0), (1010.0, 200.0), (990.0, 300.0), (1005.0, 400.0)])

        # volume>0 の bar = 注入 tick を集約した確定 bar（空 bar は build_with_no_updates 由来）。
        filled = [b for b in h.strategy.bars if float(b.volume) > 0.0]
        assert filled, "on_bar must receive at least one aggregated bar from injected ticks"
        bar = filled[0]
        assert float(bar.open) == 1000.0
        assert float(bar.high) == 1010.0
        assert float(bar.low) == 990.0
        assert float(bar.close) == 1005.0
        assert float(bar.volume) == 1000.0
    finally:
        h.close()


@pytest.mark.slow
def test_live_on_bar_spec_matches_external_only_source_differs():
    """`on_bar` に届く Live aggregation 由来 Bar の spec は EXTERNAL catalog と一致し、
    異なるのは aggregation_source（INTERNAL）だけ（§5 Bar 供給の一致）。"""
    h = _DataHarness()
    try:
        h.run_feeding([(1000.0, 100.0), (1005.0, 50.0)])
        filled = [b for b in h.strategy.bars if float(b.volume) > 0.0]
        assert filled, "expected an aggregated bar"
        live_bt = filled[0].bar_type
        external_bt = BarType.from_str("7203.TSE-1-SECOND-LAST-EXTERNAL")
        assert live_bt.spec == external_bt.spec
        assert live_bt.instrument_id == external_bt.instrument_id
        assert live_bt.aggregation_source == AggregationSource.INTERNAL
    finally:
        h.close()


# ---------------------------------------------------------------------------
# 3. controller wiring: attach が runner に tick listener を張る
# ---------------------------------------------------------------------------


class _KwargsCapturing(Strategy):
    """kwargs 形式（controller の attach 契約）。INTERNAL bar を購読する。"""

    def __init__(self, instrument_id: str, bar_type_str: str) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._bt = bar_type_str
        self.bars: list[Bar] = []

    def on_start(self) -> None:
        self.subscribe_bars(BarType.from_str(self._bt))

    def on_bar(self, bar: Bar) -> None:
        self.bars.append(bar)


class _FakeRunner:
    """tick listener registry と subscribe を記録する最小 runner stub。"""

    def __init__(self) -> None:
        self.subscribed: list[str] = []
        self.listeners: list = []

    async def subscribe(self, instrument_id: str) -> None:
        self.subscribed.append(instrument_id)

    def add_tick_listener(self, cb) -> None:
        self.listeners.append(cb)

    def remove_tick_listener(self, cb) -> None:
        if cb in self.listeners:
            self.listeners.remove(cb)


def _bg_loop():
    loop = asyncio.new_event_loop()
    t = threading.Thread(target=loop.run_forever, daemon=True)
    t.start()
    return loop, t


def _stop_bg_loop(loop, t) -> None:
    loop.call_soon_threadsafe(loop.stop)
    t.join(timeout=5)
    if not loop.is_closed():
        loop.close()


@pytest.fixture
def logged_in_adapter():
    from engine.live.mock_adapter import MockVenueAdapter

    adapter = MockVenueAdapter()
    adapter.is_logged_in = True
    adapter.set_account_snapshot(cash=10_000_000.0, buying_power=10_000_000.0)
    return adapter


def test_controller_attach_wires_tick_listener_and_feeds_data_client(logged_in_adapter):
    """attach は runner に instrument を購読させ tick listener を張る。listener 経由の
    TradesUpdate は data client → engine に届く（_seq が進む）。detach で listener を外す。"""
    from engine.live.engine_controller import NautilusLiveEngineController
    from engine.live.safety_rails import SafetyLimits, SafetyRails

    runner = _FakeRunner()
    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop,
        adapter_provider=lambda: logged_in_adapter,
        runner_provider=lambda: runner,
    )
    scenario = {"instruments": [_IID], "granularity": "Minute"}
    try:
        controller.attach(
            strategy_cls=_KwargsCapturing,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-feed0001",
            session=None,
            safety_rails=SafetyRails(SafetyLimits()),
        )
        assert runner.subscribed == [_IID]
        assert len(runner.listeners) == 1
        listener = runner.listeners[0]

        # listener を live loop 上で呼ぶ（production も LiveRunner._run = loop thread）。
        def _push(tu):
            async def _call():
                listener(tu)

            asyncio.run_coroutine_threadsafe(_call(), loop).result(timeout=5)

        _push(_tu(1, 1000.0, 100.0))
        _push(_tu(2, 1001.0, 50.0))
        assert controller._data_client._seq == 2  # 2 件とも cache 登録銘柄で feed された

        # 別銘柄の tick は無視される（_seq 不変）。
        _push(_tu(3, 1.0, 1.0, iid="9999.TSE"))
        assert controller._data_client._seq == 2

        controller.detach(nautilus_strategy_id="LIVE-feed0001")
        assert runner.listeners == []
    finally:
        _stop_bg_loop(loop, t)


def test_controller_attach_registers_scenario_universe_before_strategy_start(logged_in_adapter):
    """Issue #49: multi-symbol live strategies subscribe bars for their universe in
    on_start, so every scenario instrument must exist in Nautilus before attach starts
    the kernel."""
    from nautilus_trader.model.identifiers import InstrumentId

    from engine.live.engine_controller import NautilusLiveEngineController
    from engine.live.safety_rails import SafetyLimits, SafetyRails

    runner = _FakeRunner()
    loop, t = _bg_loop()
    controller = NautilusLiveEngineController(
        loop_provider=lambda: loop,
        adapter_provider=lambda: logged_in_adapter,
        runner_provider=lambda: runner,
    )
    scenario = {"instruments": [_IID, "9984.TSE", "1306.TSE"], "granularity": "Minute"}
    try:
        controller.attach(
            strategy_cls=_KwargsCapturing,
            scenario=scenario,
            instrument_id=_IID,
            venue="TSE",
            params={},
            nautilus_strategy_id="LIVE-univ001",
            session=None,
            safety_rails=SafetyRails(SafetyLimits(max_order_value_jpy=500_000)),
        )

        assert runner.subscribed == [_IID, "9984.TSE", "1306.TSE"]
        assert (
            controller._kernel.cache.instrument(InstrumentId.from_str("9984.TSE"))
            is not None
        )
        assert (
            controller._kernel.cache.instrument(InstrumentId.from_str("1306.TSE"))
            is not None
        )

        listener = runner.listeners[0]

        def _push(tu):
            async def _call():
                listener(tu)

            asyncio.run_coroutine_threadsafe(_call(), loop).result(timeout=5)

        _push(_tu(1, 1000.0, 100.0, iid="9984.TSE"))
        _push(_tu(2, 2000.0, 50.0, iid="1306.TSE"))
        assert controller._data_client._seq == 2

        _push(_tu(3, 1.0, 1.0, iid="9999.TSE"))
        assert controller._data_client._seq == 2

        controller.detach(nautilus_strategy_id="LIVE-univ001")
    finally:
        _stop_bg_loop(loop, t)
