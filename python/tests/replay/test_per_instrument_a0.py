"""Phase A0 contract: per_instrument multi-instrument / OhlcPoint.volume / Live depth surfacing."""
from __future__ import annotations

import asyncio

import pytest

from engine.core import DataEngine
from engine.reducer import ReducerState
from engine.models import HistoryPoint, OhlcPoint


def _engine_with_providers(providers):
    """test_multi_instrument_replay._engine_with_providers と同形（local copy）。
    先頭 provider の first tick で priming する。"""
    from engine.replay import NautilusBarsReplayProvider  # noqa: F401 (parity)
    engine = DataEngine()
    first_iid = next(iter(providers))
    first_provider = providers[first_iid]
    first_tick = first_provider.pop_next_tick()
    assert first_tick is not None
    ts, o, h, l, c, *_rest = first_tick
    ts_ms = int(ts * 1000)
    engine._rs = ReducerState(
        timestamp_ms=ts_ms, price=c, open=o, high=h, low=l,
        history=[c], history_points=[HistoryPoint(timestamp_ms=ts_ms, price=c)],
        ohlc_points=[OhlcPoint(timestamp_ms=ts_ms, open_time_ms=ts_ms, open=o, high=h, low=l, close=c)],
        max_history_len=1000,
    )
    engine._rs.per_id_close[first_iid] = c
    engine._rs.per_id_ohlc_points[first_iid] = list(engine._rs.ohlc_points)  # A0: production priming (core.py:208-210) の per-id 複製
    engine._replay_providers = dict(providers)
    engine._replay_primary_id = first_iid
    engine._mode = "replay"
    engine._replay_state = "RUNNING"
    engine._is_running = True
    return engine


class _StubProvider:
    def __init__(self, ticks):
        self._data = ticks
        self._idx = 0
    def get_next_tick(self):
        return self.pop_next_tick()
    def peek_next_tick(self):
        return self._data[self._idx] if self._idx < len(self._data) else None
    def pop_next_tick(self):
        if self._idx < len(self._data):
            t = self._data[self._idx]; self._idx += 1; return t
        return None
    def is_exhausted(self):
        return self._idx >= len(self._data)


def test_per_instrument_carries_multiple_instruments():
    """get_current_state().per_instrument に全 instrument が出る（primary/non-primary 両方）。"""
    p1 = _StubProvider([(2.0, 100.0, 110.0, 90.0, 105.0)])  # primary (priming で消費)
    p2 = _StubProvider([(2.0, 200.0, 220.0, 180.0, 210.0)])
    engine = _engine_with_providers({"A.TSE": p1, "B.TSE": p2})
    engine._advance_one_locked()  # 残り tick を drain → per_id_close 両方更新
    state = engine.get_current_state()
    assert set(state.per_instrument.keys()) == {"A.TSE", "B.TSE"}
    # A0: 各 instrument が自分の OHLC を持つ（primary も non-primary も）
    a_pts = state.per_instrument["A.TSE"].ohlc_points
    b_pts = state.per_instrument["B.TSE"].ohlc_points
    assert len(a_pts) >= 1
    assert len(b_pts) >= 1
    # per-instrument 分離の証明: A と B の最新 close は別物
    assert a_pts[-1].close == 105.0
    assert b_pts[-1].close == 210.0
    # depth は Replay では全銘柄 None
    assert state.per_instrument["A.TSE"].depth is None
    assert state.per_instrument["B.TSE"].depth is None


def test_ohlc_point_volume_concrete_through_real_bar_path():
    """6-tuple tick (..., volume) → _advance_one_locked → KlineUpdate(volume) → OhlcPoint.volume が具体値。"""
    # primary の priming tick は 5-tuple（volume なし）、drain される 2 本目を 6-tuple に
    p1 = _StubProvider([
        (1.0, 100.0, 110.0, 90.0, 100.0),            # priming（消費）
        (2.0, 100.0, 110.0, 90.0, 105.0, 1234.0),    # drain される実バー（volume=1234）
    ])
    engine = _engine_with_providers({"A.TSE": p1})
    engine._advance_one_locked()
    state = engine.get_current_state()
    pts = state.per_instrument["A.TSE"].ohlc_points
    assert pts, "primary は ohlc_points を持つはず"
    assert pts[-1].volume == pytest.approx(1234.0)
    assert pts[-1].volume is not None


def test_live_depth_surfaces_via_depth_cache_snapshot():
    """DepthUpdate を bus に流す → DepthCache.snapshot()[id] が GetState 注入と同じ DepthSnapshot 形になる。
    （GetState は depth_by_id.get(k) を per_instrument[id].depth に注入する）"""
    from engine.live.adapter import DepthLevel as AdDepthLevel, DepthUpdate
    from engine.live.event_bus import LiveEventBus
    from engine.live.depth_cache import DepthCache

    depth = DepthUpdate(
        kind="depth", instrument_id="7203", ts_ns=1_700_000_000_000_000_000,
        bids=(AdDepthLevel(price=100.0, size=3.0),),
        asks=(AdDepthLevel(price=102.0, size=2.0),),
    )

    async def scenario():
        bus = LiveEventBus()
        cache = DepthCache(bus=bus)
        await cache.start()
        await bus.publish(depth)
        await asyncio.sleep(0.05)
        snap = cache.snapshot()
        await cache.stop()
        await bus.close()
        return snap

    snap = asyncio.run(scenario())
    assert "7203" in snap
    ds = snap["7203"]
    assert [(b.price, b.size) for b in ds.bids] == [(100.0, 3.0)]
    assert [(a.price, a.size) for a in ds.asks] == [(102.0, 2.0)]
    assert ds.timestamp_ms == 1_700_000_000_000  # ts_ns // 1_000_000
