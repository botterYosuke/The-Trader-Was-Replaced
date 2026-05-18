"""LastPriceCache spec (Phase 8 follow-up: sidebar 最新価格列).

責務 (Quote 優先 / Trade fallback):
- DepthUpdate の best bid/ask から mid=(bid+ask)/2 を quote_mid として保持
- TradesUpdate.price は last_trade に保持し、quote_mid が無いときの fallback
- KlineUpdate は無視

このファイルは最初の RED 1 件のみ。実装 (engine.live.last_price_cache) は未着手。
SUT 未実装による collection error を避けるため import は setup_method 内で遅延。
"""
from __future__ import annotations

import asyncio


class TestLastPriceCacheQuoteMid:
    def setup_method(self) -> None:
        # SUT 未実装段階での collection error 回避 (memory: tdd-setup-method-deferred-import)
        from engine.live.adapter import (
            DepthLevel,
            DepthUpdate,
            KlineUpdate,
            TradesUpdate,
        )
        from engine.live.event_bus import LiveEventBus
        from engine.live.last_price_cache import LastPriceCache

        self.DepthLevel = DepthLevel
        self.DepthUpdate = DepthUpdate
        self.KlineUpdate = KlineUpdate
        self.TradesUpdate = TradesUpdate
        self.LiveEventBus = LiveEventBus
        self.LastPriceCache = LastPriceCache

    def test_snapshot_uses_quote_mid_when_depth_received(self) -> None:
        """DepthUpdate (bid=100, ask=102) 1 件 → snapshot()["7203"] == 101.0"""
        depth = self.DepthUpdate(
            kind="depth",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_000,
            bids=(self.DepthLevel(price=100.0, size=1.0),),
            asks=(self.DepthLevel(price=102.0, size=1.0),),
        )

        async def scenario() -> dict[str, float]:
            bus = self.LiveEventBus()
            cache = self.LastPriceCache(bus=bus)
            await cache.start()
            await bus.publish(depth)
            # cache が消費する余地を与える (reducer_bridge test と同じ流儀)
            await asyncio.sleep(0.05)
            snap = cache.snapshot()
            await cache.stop()
            await bus.close()
            return snap

        snap = asyncio.run(scenario())
        assert snap == {"7203": 101.0}

    def test_snapshot_falls_back_to_last_trade_when_no_depth(self) -> None:
        """TradesUpdate のみ → snapshot()["7203"] == trade.price"""
        trade = self.TradesUpdate(
            kind="trades",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_000,
            price=200.0,
            size=1.0,
            aggressor_side="buy",
        )

        async def scenario() -> dict[str, float]:
            bus = self.LiveEventBus()
            cache = self.LastPriceCache(bus=bus)
            await cache.start()
            await bus.publish(trade)
            await asyncio.sleep(0.05)
            snap = cache.snapshot()
            await cache.stop()
            await bus.close()
            return snap

        assert asyncio.run(scenario()) == {"7203": 200.0}

    def test_snapshot_prefers_quote_mid_over_last_trade_when_both_present(self) -> None:
        """Trade(200) → Depth(100/102) の順 → snapshot は quote 優先 101.0"""
        trade = self.TradesUpdate(
            kind="trades",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_000,
            price=200.0,
            size=1.0,
            aggressor_side="buy",
        )
        depth = self.DepthUpdate(
            kind="depth",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_001,
            bids=(self.DepthLevel(price=100.0, size=1.0),),
            asks=(self.DepthLevel(price=102.0, size=1.0),),
        )

        async def scenario() -> dict[str, float]:
            bus = self.LiveEventBus()
            cache = self.LastPriceCache(bus=bus)
            await cache.start()
            await bus.publish(trade)
            await bus.publish(depth)
            await asyncio.sleep(0.05)
            snap = cache.snapshot()
            await cache.stop()
            await bus.close()
            return snap

        assert asyncio.run(scenario()) == {"7203": 101.0}

    def test_snapshot_keeps_previous_quote_when_one_side_missing(self) -> None:
        """両側ありの Depth → 片側欠 Depth の順 → 前回 quote_mid を保持"""
        depth_full = self.DepthUpdate(
            kind="depth",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_000,
            bids=(self.DepthLevel(price=100.0, size=1.0),),
            asks=(self.DepthLevel(price=102.0, size=1.0),),
        )
        depth_partial = self.DepthUpdate(
            kind="depth",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_001,
            bids=(),
            asks=(self.DepthLevel(price=105.0, size=1.0),),
        )

        async def scenario() -> dict[str, float]:
            bus = self.LiveEventBus()
            cache = self.LastPriceCache(bus=bus)
            await cache.start()
            await bus.publish(depth_full)
            await bus.publish(depth_partial)
            await asyncio.sleep(0.05)
            snap = cache.snapshot()
            await cache.stop()
            await bus.close()
            return snap

        assert asyncio.run(scenario()) == {"7203": 101.0}

    def test_snapshot_returns_independent_copy(self) -> None:
        """snapshot() の戻り値を外部 mutate しても内部状態は壊れない"""
        depth = self.DepthUpdate(
            kind="depth",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_000,
            bids=(self.DepthLevel(price=100.0, size=1.0),),
            asks=(self.DepthLevel(price=102.0, size=1.0),),
        )

        async def scenario() -> tuple[dict[str, float], dict[str, float]]:
            bus = self.LiveEventBus()
            cache = self.LastPriceCache(bus=bus)
            await cache.start()
            await bus.publish(depth)
            await asyncio.sleep(0.05)
            snap1 = cache.snapshot()
            snap1["7203"] = 999.0
            snap1["9999"] = 1.0
            snap2 = cache.snapshot()
            await cache.stop()
            await bus.close()
            return snap1, snap2

        snap1, snap2 = asyncio.run(scenario())
        assert snap1 == {"7203": 999.0, "9999": 1.0}
        assert snap2 == {"7203": 101.0}

    def test_kline_update_is_ignored(self) -> None:
        """KlineUpdate のみ流す → snapshot() == {} (quote_mid/last_trade とも空)"""
        kline = self.KlineUpdate(
            kind="kline",
            instrument_id="7203",
            ts_ns=1_700_000_000_000_000_000,
            open=100.0,
            high=110.0,
            low=90.0,
            close=105.0,
            volume=1000.0,
        )

        async def scenario() -> dict[str, float]:
            bus = self.LiveEventBus()
            cache = self.LastPriceCache(bus=bus)
            await cache.start()
            await bus.publish(kline)
            await asyncio.sleep(0.05)
            snap = cache.snapshot()
            await cache.stop()
            await bus.close()
            return snap

        assert asyncio.run(scenario()) == {}
