"""TickBarAggregator — TradesUpdate (tick) を 1 本の KlineUpdate (bar) に集約する。

Phase 8 §3.7 / ADR Tick→Bar 集約。

設計:
- 進行中 bar は内部 dataclass _PartialBar で保持（KlineUpdate は frozen なので mutate 不可）。
- バケット = ts_ns // interval_ns。バケットが変わった瞬間に直前 bar を確定し emit。
- 複数分 skip しても空 bar は埋めず、直前 1 本だけ emit する。
- 同一分内 out-of-order tick: high/low/volume は集計、close は「最後着順 (= 最後に on_tick された tick の price)」。
- bar の ts_ns は「その bar の開始時刻 = bucket * interval_ns」。
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from engine.live.adapter import KlineUpdate, TradesUpdate


@dataclass
class _PartialBar:
    bucket: int
    ts_ns: int
    open: float
    high: float
    low: float
    close: float
    volume: float

    def apply(self, tick: TradesUpdate) -> None:
        if tick.price > self.high:
            self.high = tick.price
        if tick.price < self.low:
            self.low = tick.price
        self.close = tick.price
        self.volume += tick.size

    def to_kline(self, instrument_id: str) -> KlineUpdate:
        return KlineUpdate(
            kind="kline",
            instrument_id=instrument_id,
            ts_ns=self.ts_ns,
            open=self.open,
            high=self.high,
            low=self.low,
            close=self.close,
            volume=self.volume,
        )


class TickBarAggregator:
    """単一 instrument / 単一 interval の tick→bar 集約器。"""

    def __init__(self, instrument_id: str, interval_ns: int) -> None:
        if interval_ns <= 0:
            raise ValueError("interval_ns must be positive")
        self._instrument_id = instrument_id
        self._interval_ns = interval_ns
        self._current: Optional[_PartialBar] = None

    def on_tick(self, tick: TradesUpdate) -> Optional[KlineUpdate]:
        bucket = tick.ts_ns // self._interval_ns
        if self._current is None:
            self._current = _new_partial(tick, bucket, self._interval_ns)
            return None

        if bucket == self._current.bucket:
            self._current.apply(tick)
            return None

        closed = self._current.to_kline(self._instrument_id)
        self._current = _new_partial(tick, bucket, self._interval_ns)
        return closed

    def build_now(self) -> Optional[KlineUpdate]:
        if self._current is None:
            return None
        return self._current.to_kline(self._instrument_id)


def _new_partial(tick: TradesUpdate, bucket: int, interval_ns: int) -> _PartialBar:
    return _PartialBar(
        bucket=bucket,
        ts_ns=bucket * interval_ns,
        open=tick.price,
        high=tick.price,
        low=tick.price,
        close=tick.price,
        volume=tick.size,
    )
