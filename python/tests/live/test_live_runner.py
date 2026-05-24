"""LiveRunner spec (Phase 8 §3 — adapter → aggregator → event_bus pipeline).

責務 (Step 1 スコープ):
- LiveRunner.subscribe(instrument_id) で MockVenueAdapter に
  {"trades"} を subscribe し、内部に TickBarAggregator を 1 個生成する。
- adapter.events() から流れてくる TradesUpdate を on_tick に流し、
  bar が確定したら LiveEventBus.publish(KlineUpdate) する。
- 外部 consumer は LiveRunner.bus.subscribe() 経由で KlineUpdate を受け取る。

Step スコープ外（このテストでは検証しない）:
- reducer 接続 / Nautilus 型変換
- depth / 直接 kline pass-through
- 複数 instrument / 複数 interval
"""
from __future__ import annotations

import asyncio

from engine.live.adapter import (
    DepthLevel,
    DepthUpdate,
    KlineUpdate,
    TradesUpdate,
    VenueCredentials,
)
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.live_runner import LiveRunner


INTERVAL_NS = 60 * 1_000_000_000  # 1 分


async def _next_kline(it, timeout: float = 1.0) -> KlineUpdate:
    """Fix 2: bus now publishes TradesUpdate before KlineUpdate.
    Skip TradesUpdate events and return the first KlineUpdate."""
    deadline = asyncio.get_event_loop().time() + timeout
    while True:
        remaining = deadline - asyncio.get_event_loop().time()
        if remaining <= 0:
            raise asyncio.TimeoutError("timed out waiting for KlineUpdate")
        evt = await asyncio.wait_for(it.__anext__(), timeout=remaining)
        if isinstance(evt, KlineUpdate):
            return evt


def _tick(ts_ns: int, price: float, size: float = 1.0, instrument_id: str = "7203.TSE") -> TradesUpdate:
    return TradesUpdate(
        kind="trades",
        instrument_id=instrument_id,
        ts_ns=ts_ns,
        price=price,
        size=size,
        aggressor_side="buy",
    )


def test_live_runner_aggregates_ticks_into_kline_via_bus() -> None:
    """RED: LiveRunner が tick→bar 集約結果を LiveEventBus 経由で publish する。

    シナリオ:
      1. MockVenueAdapter と LiveRunner(interval_ns=1min) を作る。
      2. runner.subscribe("7203.TSE") → adapter は {"trades"} を購読し、
         内部に TickBarAggregator が 1 個できる。
      3. consumer = runner.bus.subscribe() で AsyncIterator を取得。
      4. runner.start() で background task が adapter.events() を消費開始。
      5. adapter.inject_tick で同一分 (bucket 28333333) に 2 本、
         次の分 (bucket 28333334) に 1 本注入。
      6. 次の分の 1 本目を受け取った時点で「直前 bar」が確定し、
         bus に KlineUpdate が 1 件 publish されることを検証。
      7. runner.stop() で background task を綺麗に止め、bus を close。

    期待:
      - consumer から取れる最初の KlineUpdate が次の通り:
          instrument_id == "7203.TSE"
          ts_ns         == 28333333 * INTERVAL_NS   # bucket 開始時刻
          open          == 100.0
          high          == 110.0
          low           ==  90.0
          close         ==  90.0                    # 同分内 最終 tick
          volume        ==   3.0
    """
    base_ns = 28333333 * INTERVAL_NS  # 区切りの良い分境界

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        consumer = runner.bus.subscribe()
        await runner.start()

        # 同一分に 2 本（open=100, high=110, close 暫定=110）
        adapter.inject_tick(_tick(base_ns + 0,                price=100.0, size=1.0))
        adapter.inject_tick(_tick(base_ns + 10_000_000_000,   price=110.0, size=1.0))
        # 次の分の 1 本目 → 直前 bar 確定 emit のトリガ（close=90 は次 bar 側）
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS,      price= 90.0, size=1.0))

        # 直前 bar が来るのを 1 件だけ待つ (TradesUpdate は _next_kline でスキップ)
        try:
            it = consumer.__aiter__()
            evt = await _next_kline(it, timeout=1.0)
        finally:
            await runner.stop()

        return evt

    bar = asyncio.run(scenario())

    assert bar.instrument_id == "7203.TSE"
    assert bar.ts_ns         == 28333333 * INTERVAL_NS
    assert bar.open          == 100.0
    assert bar.high          == 110.0
    # 同分内 tick の close は「最後着順 price」= 110.0（次分 tick は別 bar）
    assert bar.close         == 110.0
    assert bar.low           == 100.0
    assert bar.volume        ==   2.0


def test_live_runner_pushes_partial_bar_snapshot() -> None:
    """Phase 10 Step 8: partial_push_interval_s>0 のとき、確定前でも進行中バーの
    スナップショット（`build_now()`）を一定間隔で bus に publish する（UI 用 partial bar）。

    シナリオ:
      1. LiveRunner(partial_push_interval_s=0.2) を作り subscribe + start。
      2. 同一分バケットに 2 本注入（バケット roll 無し → 確定バーは emit されない）。
      3. それでも partial push タスクが in-progress バー（open=100/high=110/low=100/
         close=110/volume=2、ts_ns=バケット開始）を KlineUpdate として publish する。
    """
    base_ns = 28333333 * INTERVAL_NS

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(
            adapter=adapter, interval_ns=INTERVAL_NS, partial_push_interval_s=0.2
        )
        await adapter.login(
            VenueCredentials(credentials_source="env", environment_hint="demo")
        )
        await runner.subscribe("7203.TSE")
        consumer = runner.bus.subscribe()
        await runner.start()

        # 同一分に 2 本だけ（roll しないので on_tick は確定バーを emit しない）。
        adapter.inject_tick(_tick(base_ns + 0, price=100.0, size=1.0))
        adapter.inject_tick(_tick(base_ns + 10_000_000_000, price=110.0, size=1.0))

        try:
            it = consumer.__aiter__()
            # 唯一来る KlineUpdate は partial push のスナップショット。
            evt = await _next_kline(it, timeout=2.0)
        finally:
            await runner.stop()
        return evt

    bar = asyncio.run(scenario())

    assert bar.instrument_id == "7203.TSE"
    assert bar.ts_ns == 28333333 * INTERVAL_NS  # バケット開始時刻
    assert bar.open == 100.0
    assert bar.high == 110.0
    assert bar.low == 100.0
    assert bar.close == 110.0
    assert bar.volume == 2.0


def test_live_runner_passes_depth_update_through_bus() -> None:
    """RED (Step 2a): DepthUpdate は aggregation を経由せず bus に pass-through される。

    シナリオ:
      1. MockVenueAdapter + LiveRunner(interval_ns=1min)
      2. runner.subscribe("7203.TSE") で adapter に trades + depth 両方を購読
         （LiveRunner は depth pass-through のため depth も購読する）
      3. consumer = runner.bus.subscribe()
      4. runner.start()
      5. adapter.emit_depth_snapshot で DepthUpdate を 1 件注入
      6. consumer から取れる最初の event が DepthUpdate そのものであること

    Step 1 と違い、aggregator は経由しないので即時 publish される。
    """
    ts_ns = 28333333 * INTERVAL_NS

    async def scenario() -> DepthUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        consumer = runner.bus.subscribe()
        await runner.start()

        adapter.emit_depth_snapshot(
            "7203.TSE",
            ts_ns,
            bids=[DepthLevel(price=100.0, size=10.0)],
            asks=[DepthLevel(price=101.0, size=5.0)],
        )

        try:
            it = consumer.__aiter__()
            evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        finally:
            await runner.stop()

        assert isinstance(evt, DepthUpdate)
        return evt

    depth = asyncio.run(scenario())

    assert depth.instrument_id == "7203.TSE"
    assert depth.ts_ns         == ts_ns
    assert depth.bids[0].price == 100.0
    assert depth.bids[0].size  == 10.0
    assert depth.asks[0].price == 101.0
    assert depth.asks[0].size  == 5.0


def test_live_runner_passes_direct_kline_update_through_bus() -> None:
    """RED (Step 2b): venue から直接届く KlineUpdate は aggregator を迂回して bus に流す。

    シナリオ:
      - aggregator は tick からの集約用。venue が既に集約済み bar を送ってきた場合は
        aggregator を一切経由せず、そのまま pass-through する。
      - adapter.inject_tick(KlineUpdate(...)) で 1 本注入し、
        consumer から取れる最初の event がその KlineUpdate そのもの（同一値）であること。
    """
    ts_ns = 28333333 * INTERVAL_NS
    bar_in = KlineUpdate(
        kind="kline",
        instrument_id="7203.TSE",
        ts_ns=ts_ns,
        open=100.0, high=110.0, low=95.0, close=105.0, volume=42.0,
    )

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        consumer = runner.bus.subscribe()
        await runner.start()

        adapter.inject_tick(bar_in)

        try:
            it = consumer.__aiter__()
            evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        finally:
            await runner.stop()

        assert isinstance(evt, KlineUpdate)
        return evt

    bar = asyncio.run(scenario())
    assert bar == bar_in


def test_live_runner_aggregates_multiple_instruments_independently() -> None:
    """RED (Step 2c): 複数 instrument を同時に subscribe したとき、
    各 aggregator が独立に bar を確定して bus に publish する。

    シナリオ:
      - "7203.TSE" と "9984.TSE" を subscribe
      - 同一分に両方 1 本ずつ tick → 次分 tick で両方の直前 bar 確定
      - bus から 2 件 KlineUpdate を取得し、instrument_id 別に振り分けて検証
    """
    base_ns = 28333333 * INTERVAL_NS

    async def scenario() -> dict[str, KlineUpdate]:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")
        await runner.subscribe("9984.TSE")

        consumer = runner.bus.subscribe()
        await runner.start()

        adapter.inject_tick(_tick(base_ns + 0, price=100.0, size=1.0, instrument_id="7203.TSE"))
        adapter.inject_tick(_tick(base_ns + 0, price=200.0, size=2.0, instrument_id="9984.TSE"))
        # 次分 tick で両 instrument の直前 bar が確定
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS, price=101.0, size=1.0, instrument_id="7203.TSE"))
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS, price=201.0, size=1.0, instrument_id="9984.TSE"))

        bars: dict[str, KlineUpdate] = {}
        try:
            it = consumer.__aiter__()
            # Fix 2: skip TradesUpdate, collect 2 KlineUpdates
            while len(bars) < 2:
                evt = await _next_kline(it, timeout=1.0)
                bars[evt.instrument_id] = evt
        finally:
            await runner.stop()
        return bars

    bars = asyncio.run(scenario())
    assert set(bars.keys()) == {"7203.TSE", "9984.TSE"}
    assert bars["7203.TSE"].open  == 100.0
    assert bars["7203.TSE"].close == 100.0
    assert bars["7203.TSE"].volume == 1.0
    assert bars["7203.TSE"].ts_ns == base_ns
    assert bars["9984.TSE"].open  == 200.0
    assert bars["9984.TSE"].close == 200.0
    assert bars["9984.TSE"].volume == 2.0
    assert bars["9984.TSE"].ts_ns == base_ns


def test_live_runner_supports_multiple_intervals_per_instrument() -> None:
    """RED (Step 2d): LiveRunner(intervals_ns=[1m, 2m]) で 1 tick が両方の
    aggregator に流れ、それぞれの境界で独立に bar が確定する。

    シナリオ:
      - intervals = [1min, 2min]
      - tick at t=base, t=base+1min, t=base+2min
      - 期待:
        * 1m aggregator: [base, base+1min) と [base+1min, base+2min) の 2 本確定
        * 2m aggregator: [base, base+2min) の 1 本確定
      - bar の interval は KlineUpdate に持たないため、(ts_ns, volume) の組で識別する:
        * (base,       1.0) = 1m@base   open=close=100
        * (base+1min,  1.0) = 1m@base+1min open=close=110
        * (base,       2.0) = 2m@base   open=100 close=110
    """
    INTERVAL_2M = 2 * INTERVAL_NS
    base_ns = 14166666 * INTERVAL_2M  # 1m と 2m 両方の bucket 境界

    async def scenario() -> list[KlineUpdate]:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, intervals_ns=[INTERVAL_NS, INTERVAL_2M])

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        consumer = runner.bus.subscribe()
        await runner.start()

        adapter.inject_tick(_tick(base_ns + 0,             price=100.0, size=1.0))
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS,   price=110.0, size=1.0))
        adapter.inject_tick(_tick(base_ns + INTERVAL_2M,   price=120.0, size=1.0))

        bars: list[KlineUpdate] = []
        try:
            it = consumer.__aiter__()
            # Fix 2: skip TradesUpdate, collect 3 KlineUpdates
            while len(bars) < 3:
                evt = await _next_kline(it, timeout=1.0)
                bars.append(evt)
        finally:
            await runner.stop()
        return bars

    bars = asyncio.run(scenario())
    keys = {(b.ts_ns, b.volume) for b in bars}
    assert keys == {
        (base_ns,                  1.0),  # 1m @ base
        (base_ns + INTERVAL_NS,    1.0),  # 1m @ base+1m
        (base_ns,                  2.0),  # 2m @ base
    }
    # 2m bar の close は同分内の最後着順 (110 — 120 は次 2m bucket 開始)
    bar_2m = next(b for b in bars if b.ts_ns == base_ns and b.volume == 2.0)
    assert bar_2m.open  == 100.0
    assert bar_2m.close == 110.0
    assert bar_2m.high  == 110.0
    assert bar_2m.low   == 100.0


def test_live_runner_accepts_single_interval_int_for_backwards_compat() -> None:
    """interval_ns=int (Step 1 シグネチャ) でも従来通り動作する。"""
    base_ns = 28333333 * INTERVAL_NS

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")
        consumer = runner.bus.subscribe()
        await runner.start()
        adapter.inject_tick(_tick(base_ns + 0,            price=100.0))
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS,  price=110.0))
        try:
            it = consumer.__aiter__()
            evt = await _next_kline(it, timeout=1.0)
        finally:
            await runner.stop()
        return evt

    bar = asyncio.run(scenario())
    assert bar.ts_ns  == base_ns
    assert bar.open   == 100.0
    assert bar.close  == 100.0


def test_live_runner_subscribe_after_start_does_not_drop_first_tick() -> None:
    """RED: start() 後に subscribe() を呼んだとき、adapter.subscribe 完了直後
    （aggregator 登録の前）に届いた tick が drop される race を観測する。

    LiveRunner.subscribe() は
        await self._adapter.subscribe(...)        # suspend point
        self._aggregators[instrument_id] = [...]  # 登録
    の順なので、suspend 中に background _run task が tick を読み、
    _aggregators.get(instrument_id) が None で握り潰される可能性がある。

    再現方法:
      - adapter.subscribe を「親 subscribe 完了 → 同一分 tick 2 本 + 次分 1 本
        を inject_tick → return」する wrapper に差し替える。
      - LiveRunner.start() → runner.subscribe("7203.TSE") の順に呼ぶ。
      - 期待: 1 件目の KlineUpdate が open=100, close=110, volume=2.0
        （= subscribe 中に inject された 2 本が aggregator に届いた証拠）。
      - 現実装では timeout か、close/volume が次分 tick 1 本ぶんしか入らず FAIL。
    """
    base_ns = 28333333 * INTERVAL_NS

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))

        # adapter.subscribe をラップ: 親の subscribe が return した直後
        # （= LiveRunner.subscribe 内で aggregator が登録される前）に
        # 同一分 2 本 + 次分 1 本を queue に積む。
        original_subscribe = adapter.subscribe

        async def racing_subscribe(instrument_id: str, channels):
            await original_subscribe(instrument_id, channels)
            adapter.inject_tick(_tick(base_ns + 0,              price=100.0, size=1.0))
            adapter.inject_tick(_tick(base_ns + 10_000_000_000, price=110.0, size=1.0))
            adapter.inject_tick(_tick(base_ns + INTERVAL_NS,    price= 90.0, size=1.0))
            # 強制的に background _run に制御を渡し、aggregator 登録前に
            # tick を pop させて drop race を発火させる。
            await asyncio.sleep(0)

        adapter.subscribe = racing_subscribe  # type: ignore[assignment]

        consumer = runner.bus.subscribe()
        await runner.start()
        await runner.subscribe("7203.TSE")

        try:
            it = consumer.__aiter__()
            evt = await _next_kline(it, timeout=1.0)
        finally:
            await runner.stop()

        return evt

    bar = asyncio.run(scenario())

    assert bar.instrument_id == "7203.TSE"
    assert bar.ts_ns         == base_ns
    assert bar.open          == 100.0
    assert bar.high          == 110.0
    assert bar.low           == 100.0
    assert bar.close         == 110.0
    assert bar.volume        ==   2.0


def test_live_runner_can_restart_after_run_task_dies_with_exception() -> None:
    """RED (サイクル 2): adapter.events() が例外を投げて _run task が die した後、
    再度 start() を呼べば新しい _run task が立ち上がり、再注入された tick が
    bus に流れること。さらに die 原因の例外が runner.last_error から取得できる。

    現実装の問題:
      - _run が例外で死ぬと self._task は done 状態のまま残るので、
        2 回目の start() は `if self._task is not None: return` で no-op になり、
        runner は永久に events() を読まなくなる（silent dead）。
      - 例外が外から観測できない（呼び元はなぜ止まったか分からない）。

    案 C の仕様:
      - start(): self._task is None or self._task.done() なら新規 task を作る。
      - _run の例外（CancelledError 以外）は self._last_error に保存し、
        runner.last_error プロパティ（読み取り専用）で取得できる。
      - bus への error publish はしない（LiveEvent union は触らない）。
    """
    base_ns = 28333333 * INTERVAL_NS
    boom = RuntimeError("adapter stream blew up")

    async def scenario() -> tuple[KlineUpdate, BaseException | None]:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)

        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        # adapter.events() を「1 回 yield せず即例外を投げる」async generator に差し替える。
        async def exploding_events():
            raise boom
            yield  # pragma: no cover  (generator にするためのダミー)

        adapter.events = exploding_events  # type: ignore[assignment]

        await runner.start()
        # _run が例外で死ぬまで yield を回す
        for _ in range(5):
            await asyncio.sleep(0)

        assert runner._task is not None and runner._task.done(), \
            "precondition: _run task should have died from the injected exception"
        captured_error = runner.last_error
        assert captured_error is boom, \
            f"last_error should expose the cause exception, got {captured_error!r}"

        # ここから restart: events() を正常な queue ベースに戻し、
        # start() を再度呼べば新しい _run task が立ち上がること。
        adapter.events = MockVenueAdapter.events.__get__(adapter, MockVenueAdapter)  # type: ignore[assignment]

        consumer = runner.bus.subscribe()
        await runner.start()  # 現実装ではここが no-op → 以下の tick が永遠に消費されず timeout

        adapter.inject_tick(_tick(base_ns + 0,            price=100.0, size=1.0))
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS,  price=110.0, size=1.0))

        try:
            it = consumer.__aiter__()
            evt = await _next_kline(it, timeout=1.0)
        finally:
            await runner.stop()

        return evt, captured_error

    bar, err = asyncio.run(scenario())
    assert err is boom
    assert bar.instrument_id == "7203.TSE"
    assert bar.ts_ns         == base_ns
    assert bar.open          == 100.0
    assert bar.close         == 100.0
    assert bar.volume        == 1.0


def test_live_runner_drops_depth_for_unsubscribed_instrument() -> None:
    """RED (F1): runner.subscribe していない instrument の DepthUpdate は bus に流さない。

    シナリオ:
      - runner.subscribe("7203.TSE") のみ実施。
      - mock の adapter には別途 "9984.TSE" を直接 subscribe（実 venue で global stream
        の別銘柄 frame や unsubscribe 直後の残留 frame が来た状況を再現）。
      - "9984.TSE" の DepthUpdate を inject → bus には届かない
      - 続けて "7203.TSE" の DepthUpdate を inject → bus に届く
      - bus から取れる最初の event は 7203 のものであることで gating を確認
    """
    ts_ns = 28333333 * INTERVAL_NS

    async def scenario() -> DepthUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")
        # adapter level でのみ 9984 を subscribe（runner._aggregators には載せない）
        await adapter.subscribe("9984.TSE", {"depth"})

        consumer = runner.bus.subscribe()
        await runner.start()

        adapter.emit_depth_snapshot(
            "9984.TSE", ts_ns,
            bids=[DepthLevel(price=200.0, size=1.0)],
            asks=[DepthLevel(price=201.0, size=1.0)],
        )
        adapter.emit_depth_snapshot(
            "7203.TSE", ts_ns,
            bids=[DepthLevel(price=100.0, size=1.0)],
            asks=[DepthLevel(price=101.0, size=1.0)],
        )

        try:
            it = consumer.__aiter__()
            evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        finally:
            await runner.stop()

        assert isinstance(evt, DepthUpdate)
        return evt

    depth = asyncio.run(scenario())
    assert depth.instrument_id == "7203.TSE"


def test_trades_update_published_to_bus() -> None:
    """Fix 2: TradesUpdate が来たとき、aggregator に渡す前後に関わらず
    bus に publish される（bar が確定しない場合でも TradesUpdate 自体が流れる）。"""
    ts_ns = 28333333 * INTERVAL_NS

    async def scenario() -> TradesUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        consumer = runner.bus.subscribe()
        await runner.start()

        # 1 本だけ inject (bar 確定には至らない)
        adapter.inject_tick(_tick(ts_ns + 0, price=100.0, size=1.0))

        try:
            it = consumer.__aiter__()
            evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        finally:
            await runner.stop()

        # TradesUpdate が bus に publish されていることを確認
        assert isinstance(evt, TradesUpdate), (
            f"Expected TradesUpdate on bus, got {type(evt).__name__}"
        )
        return evt

    trade = asyncio.run(scenario())
    assert trade.instrument_id == "7203.TSE"
    assert trade.price == 100.0
    assert trade.size == 1.0


def test_live_runner_drops_direct_kline_for_unsubscribed_instrument() -> None:
    """RED (F1): runner.subscribe していない instrument の venue 直送 KlineUpdate も
    bus に流さない。シナリオは depth 版と同形。"""
    ts_ns = 28333333 * INTERVAL_NS
    foreign = KlineUpdate(
        kind="kline", instrument_id="9984.TSE",
        ts_ns=ts_ns, open=200.0, high=200.0, low=200.0, close=200.0, volume=1.0,
    )
    own = KlineUpdate(
        kind="kline", instrument_id="7203.TSE",
        ts_ns=ts_ns, open=100.0, high=100.0, low=100.0, close=100.0, volume=1.0,
    )

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")
        await adapter.subscribe("9984.TSE", {"trades"})

        consumer = runner.bus.subscribe()
        await runner.start()

        adapter.inject_tick(foreign)
        adapter.inject_tick(own)

        try:
            it = consumer.__aiter__()
            evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        finally:
            await runner.stop()

        assert isinstance(evt, KlineUpdate)
        return evt

    bar = asyncio.run(scenario())
    assert bar.instrument_id == "7203.TSE"
    assert bar == own


# --- Post-merge fix tests ----------------------------------------------------

def test_is_logged_in_defaults_to_false_when_adapter_lacks_attribute() -> None:
    """MEDIUM-2: adapter that does not expose `is_logged_in` must be treated
    as NOT logged in (deny-by-default)."""
    class _NoAttrAdapter:
        venue_id = "MOCK"
        async def login(self, creds): pass
        async def logout(self): pass
        async def fetch_instruments(self): return []
        async def subscribe(self, instrument_id, channels): pass
        async def unsubscribe(self, instrument_id): pass
        async def events(self):
            if False:
                yield  # pragma: no cover

    adapter = _NoAttrAdapter()
    runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
    assert runner.is_logged_in() is False


def test_is_logged_in_true_when_adapter_exposes_true() -> None:
    """Sanity: when adapter sets is_logged_in=True, runner reports True."""
    adapter = MockVenueAdapter()
    adapter.is_logged_in = True
    runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
    assert runner.is_logged_in() is True


def test_live_runner_restart_after_stop_still_publishes_events() -> None:
    """MEDIUM-4: LiveRunner.stop() must NOT close the bus.
    start → stop → start must work and events must keep flowing on the same bus."""
    base_ns = 28333333 * INTERVAL_NS

    async def scenario() -> KlineUpdate:
        adapter = MockVenueAdapter()
        runner = LiveRunner(adapter=adapter, interval_ns=INTERVAL_NS)
        await adapter.login(VenueCredentials(
            credentials_source="env", environment_hint="demo",
        ))
        await runner.subscribe("7203.TSE")

        await runner.start()
        await runner.stop()

        # After stop(), the bus must still be usable and a new start() must
        # resume background consumption.
        assert runner.bus._closed is False, "stop() must NOT close the bus"

        consumer = runner.bus.subscribe()
        await runner.start()
        adapter.inject_tick(_tick(base_ns + 0,           price=100.0, size=1.0))
        adapter.inject_tick(_tick(base_ns + INTERVAL_NS, price=110.0, size=1.0))

        try:
            it = consumer.__aiter__()
            evt = await _next_kline(it, timeout=1.0)
        finally:
            await runner.aclose()
        return evt

    bar = asyncio.run(scenario())
    assert bar.open == 100.0
    assert bar.close == 100.0
    assert bar.volume == 1.0


def test_fetch_instruments_blocking_cancels_underlying_on_timeout() -> None:
    """Issue #32: blocking fetch が timeout したら scheduled coroutine をキャンセルし、
    遅い venue download を orphan task として残さない。"""
    import concurrent.futures
    import threading

    import pytest

    loop = asyncio.new_event_loop()
    t = threading.Thread(target=loop.run_forever, daemon=True)
    t.start()
    cancelled = threading.Event()

    class SlowAdapter:
        is_logged_in = True
        venue_id = "STUB"

        async def fetch_instruments(self):
            try:
                await asyncio.sleep(10)
            except asyncio.CancelledError:
                cancelled.set()
                raise
            return []

    runner = LiveRunner(adapter=SlowAdapter(), interval_ns=INTERVAL_NS)
    runner._loop = loop
    try:
        with pytest.raises(concurrent.futures.TimeoutError):
            runner.fetch_instruments_blocking(timeout=0.1)
        assert cancelled.wait(timeout=2.0), "timeout 時に下層 coroutine がキャンセルされること"
    finally:
        loop.call_soon_threadsafe(loop.stop)
