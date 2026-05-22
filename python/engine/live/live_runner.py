"""LiveRunner — adapter → aggregator → event_bus pipeline (Phase 8 §3, Step 1+2).

責務:
- subscribe(instrument_id): adapter に {"trades", "depth"} を購読し、
  intervals_ns で指定された各 interval に対して TickBarAggregator を生成する。
- start(): adapter.events() を消費する background task を起動。
  - TradesUpdate → 当該 instrument の全 aggregator に on_tick、確定 bar を bus.publish
  - DepthUpdate / KlineUpdate (venue 直送) → そのまま bus.publish (aggregator 迂回)
- stop(): background task を cancel して await し、bus を close。

Step 2 で追加:
- DepthUpdate pass-through
- venue 直送 KlineUpdate pass-through (集約済み bar を venue が送ってくる経路)
- 複数 instrument (subscribe を複数回呼べる)
- 複数 interval (LiveRunner(intervals_ns=[60s, 300s, ...]))

Step スコープ外:
- reducer / DataEngine 接続（別途 Step 2e で converter を作る）
"""
from __future__ import annotations

import asyncio
import logging
from typing import Callable, Iterable, Optional

from engine.live.adapter import (
    DepthUpdate,
    InstrumentId,
    KlineUpdate,
    LiveVenueAdapter,
    TradesUpdate,
)
from engine.live.aggregator import TickBarAggregator
from engine.live.event_bus import LiveEventBus

log = logging.getLogger(__name__)


class LiveRunner:
    def __init__(
        self,
        adapter: LiveVenueAdapter,
        interval_ns: Optional[int] = None,
        intervals_ns: Optional[Iterable[int]] = None,
        partial_push_interval_s: float = 0.0,
    ) -> None:
        intervals = _normalize_intervals(interval_ns, intervals_ns)
        self._adapter = adapter
        self._intervals_ns: tuple[int, ...] = intervals
        # 各 instrument は interval ごとに 1 個の aggregator を持つ
        self._aggregators: dict[InstrumentId, list[TickBarAggregator]] = {}
        self.bus: LiveEventBus = LiveEventBus()
        self._task: Optional[asyncio.Task[None]] = None
        self._last_error: Optional[BaseException] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None  # D10: set by server_grpc
        # Phase 10 Step 8: 生 TradesUpdate を横取りする listener（Nautilus aggregation へ
        # tick を流す engine_controller が登録する）。bus 経路（UI 用）とは別系統。
        self._tick_listeners: list[Callable[[TradesUpdate], None]] = []
        # Phase 10 Step 8: UI 用 partial bar push（build_now() を一定間隔で bus に publish）。
        # 0 / None なら無効（既存テストの既定）。production は server_grpc が 1.0s を渡す。
        self._partial_push_interval_s: float = float(partial_push_interval_s or 0.0)
        self._partial_task: Optional[asyncio.Task[None]] = None
        # 直近に push した partial bar（(instrument_id, interval 順位) → KlineUpdate）。
        # 静かな相場で同一スナップショットを毎秒 publish して UI を溢れさせないための
        # 変更検出ガード（Step 8 efficiency review）。
        self._last_partial: dict[tuple, KlineUpdate] = {}

    async def subscribe(self, instrument_id: InstrumentId) -> None:
        # idempotent: 既に登録済みなら何もしない
        if instrument_id in self._aggregators:
            return
        # 先に aggregator を登録してから adapter に subscribe する。
        # こうすることで start() 後に subscribe() が呼ばれた場合でも
        # adapter.subscribe() 完了前に到着した最初の tick を取りこぼさない。
        self._aggregators[instrument_id] = [
            TickBarAggregator(instrument_id=instrument_id, interval_ns=iv)
            for iv in self._intervals_ns
        ]
        try:
            await self._adapter.subscribe(instrument_id, {"trades", "depth"})
        except BaseException:
            # adapter 側で失敗したら登録を巻き戻す
            self._aggregators.pop(instrument_id, None)
            raise

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        # idempotent: 未登録なら何もしない
        if instrument_id not in self._aggregators:
            return
        # 先に adapter に通知してから内部 state を落とす。
        # adapter.unsubscribe が失敗したら _aggregators は残す
        # (再試行可能 / 状態の真実は adapter 側)。
        await self._adapter.unsubscribe(instrument_id)
        self._aggregators.pop(instrument_id, None)
        # 進行中バーの変更検出キャッシュから当該銘柄の dead key を落とす。
        for key in [k for k in self._last_partial if k[0] == instrument_id]:
            self._last_partial.pop(key, None)

    def add_tick_listener(self, listener: Callable[[TradesUpdate], None]) -> None:
        """生 `TradesUpdate` を受け取る listener を登録する (Step 8)。

        各 listener は `_run` の中で（live loop thread 上で）同期呼び出しされる。
        engine_controller がここに登録し、tick を Nautilus `TradeTick` 化して
        `LiveDataEngine` に注入する（戦略 `on_bar` への bar 供給経路）。冪等。
        """
        if listener not in self._tick_listeners:
            self._tick_listeners.append(listener)

    def remove_tick_listener(self, listener: Callable[[TradesUpdate], None]) -> None:
        """登録済み tick listener を外す（未登録なら no-op）。detach で呼ぶ。"""
        try:
            self._tick_listeners.remove(listener)
        except ValueError:
            pass

    async def start(self) -> None:
        if self._task is not None and not self._task.done():
            return
        self._last_error = None
        self._task = asyncio.create_task(self._run())
        if self._partial_push_interval_s > 0.0 and (
            self._partial_task is None or self._partial_task.done()
        ):
            self._partial_task = asyncio.create_task(self._partial_push())

    def _is_subscribed(self, instrument_id: InstrumentId) -> bool:
        return instrument_id in self._aggregators

    async def _run(self) -> None:
        try:
            async for evt in self._adapter.events():
                # 未購読 instrument の event は一切流さない（実 adapter が global stream
                # の別銘柄 frame や unsubscribe 直後の残留 frame を出してきた場合の防衛線、§9.9 ADR）
                if not self._is_subscribed(evt.instrument_id):
                    continue
                if isinstance(evt, TradesUpdate):
                    # Fix 2: publish the raw TradesUpdate so LastPriceCache can
                    # use trade fallback pricing, then also run aggregation.
                    await self.bus.publish(evt)
                    for agg in self._aggregators[evt.instrument_id]:
                        closed = agg.on_tick(evt)
                        if closed is not None:
                            await self.bus.publish(closed)
                    # Step 8: Nautilus aggregation など外部 consumer に生 tick を渡す。
                    # listener は同期・best-effort（1 つが落ちても pipeline は止めない）。
                    for listener in self._tick_listeners:
                        try:
                            listener(evt)
                        except Exception:  # noqa: BLE001
                            log.exception("tick listener failed")
                elif isinstance(evt, (DepthUpdate, KlineUpdate)):
                    await self.bus.publish(evt)
        except asyncio.CancelledError:
            return
        except BaseException as exc:
            self._last_error = exc
            return

    async def _partial_push(self) -> None:
        """進行中バーのスナップショット（`build_now()`）を一定間隔で bus に publish する。

        UI 用の partial bar（未確定バー）経路（§2.3 / Step 8）。Strategy への bar 供給とは
        別系統で、確定バーは `_run` の `on_tick` が emit する。bus が close 済みなら静かに終了。
        """
        try:
            while True:
                await asyncio.sleep(self._partial_push_interval_s)
                # items() のスナップショットで回す: publish の await 中に subscribe/
                # unsubscribe が _aggregators を変えても "changed size" で死なせない。
                for instrument_id, aggs in list(self._aggregators.items()):
                    for idx, agg in enumerate(aggs):
                        kline = agg.build_now()
                        if kline is None:
                            continue
                        key = (instrument_id, idx)
                        # 前回と同一スナップショット（新しい tick 無し）なら push しない。
                        if self._last_partial.get(key) == kline:
                            continue
                        self._last_partial[key] = kline
                        await self.bus.publish(kline)
        except asyncio.CancelledError:
            return
        except Exception:  # noqa: BLE001 — bus close 後など。push 経路の失敗で runner を壊さない
            log.warning("partial bar push task stopped on error", exc_info=True)
            return

    async def stop(self) -> None:
        # Does NOT close the bus — stop() is reversible; start() can re-arm on
        # the same bus. Use aclose() when discarding the runner entirely.
        if self._partial_task is not None:
            self._partial_task.cancel()
            try:
                await self._partial_task
            except asyncio.CancelledError:
                pass
            self._partial_task = None
        # 再 start 時に古い snapshot で初回 push を取りこぼさないようガードを空にする。
        self._last_partial.clear()
        if self._task is not None:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

    async def aclose(self) -> None:
        """Explicit shutdown: stop background task, close the bus, and logout adapter.
        Use this when the runner is truly being discarded (not re-armed)."""
        await self.stop()
        await self.bus.close()
        try:
            await self._adapter.logout()
        except Exception:  # noqa: BLE001 — best-effort; orders-poll leak is the real risk (issue #16)
            log.exception("adapter.logout() failed during aclose")

    @property
    def last_error(self) -> Optional[BaseException]:
        return self._last_error

    # D10: adapter exposure for VenueLogin RPC

    @property
    def adapter(self) -> "LiveVenueAdapter":
        """Expose the underlying adapter for VenueLogin RPC (D10)."""
        return self._adapter

    def is_logged_in(self) -> bool:
        """D10: Return True if adapter is logged in and bus is alive.

        NOTE: field name is `self.bus` (public), NOT `self._bus`.
        """
        return getattr(self._adapter, "is_logged_in", False) and self.bus is not None

    @property
    def venue_id(self) -> str:
        """Phase 9 Step 9: adapter の venue id（instruments store のキー）。"""
        return getattr(self._adapter, "venue_id", "") or ""

    def fetch_instruments_blocking(self, timeout: float = 5.0):
        """D10: Fetch instruments from adapter synchronously (from gRPC thread).

        Requires _loop to be set (via _ensure_live_loop in server_grpc).
        """
        if self._loop is None:
            raise RuntimeError("LiveRunner._loop not set; call _ensure_live_loop first")
        fut = asyncio.run_coroutine_threadsafe(self._adapter.fetch_instruments(), self._loop)
        return fut.result(timeout=timeout)

    def subscribed_ids(self) -> set:
        """D20: Return the set of currently subscribed instrument IDs."""
        return set(self._aggregators.keys())


def _normalize_intervals(
    interval_ns: Optional[int],
    intervals_ns: Optional[Iterable[int]],
) -> tuple[int, ...]:
    if interval_ns is None and intervals_ns is None:
        raise ValueError("either interval_ns or intervals_ns must be provided")
    if interval_ns is not None and intervals_ns is not None:
        raise ValueError("specify only one of interval_ns or intervals_ns")
    if intervals_ns is not None:
        result = tuple(int(iv) for iv in intervals_ns)
        if not result:
            raise ValueError("intervals_ns must not be empty")
    else:
        result = (int(interval_ns),)  # type: ignore[arg-type]
    for iv in result:
        if iv <= 0:
            raise ValueError("interval_ns must be positive")
    return result
