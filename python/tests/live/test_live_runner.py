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

from engine.live.adapter import KlineUpdate, TradesUpdate
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.live_runner import LiveRunner


INTERVAL_NS = 60 * 1_000_000_000  # 1 分


def _tick(ts_ns: int, price: float, size: float = 1.0) -> TradesUpdate:
    return TradesUpdate(
        kind="trades",
        instrument_id="7203.TSE",
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

        await adapter.login(__import__("engine.live.adapter", fromlist=["VenueCredentials"]).VenueCredentials(
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

        # 直前 bar が来るのを 1 件だけ待つ
        try:
            it = consumer.__aiter__()
            evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        finally:
            await runner.stop()

        assert isinstance(evt, KlineUpdate)
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
