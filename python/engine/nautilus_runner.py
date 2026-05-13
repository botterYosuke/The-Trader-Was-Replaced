"""
Nautilus replay runner layer.

Responsibility split:
  adapter : Nautilus object  -> ReplayEvent
  runner  : event source     -> adapter -> ReplayEventSink
  core    : ReplayEvent      -> ReducerState

The runner enforces ReplayTimeUpdated → data-event ordering for every tick so
callers don't have to think about it.
"""

from typing import Iterable, Protocol

from .nautilus_adapter import (
    bar_to_kline_update,
    timestamp_ns_to_replay_time_updated,
    trade_to_trade_update,
)
from .reducer import ReplayEvent


class ReplayEventSink(Protocol):
    def apply_replay_event(self, event: ReplayEvent) -> None: ...


class NautilusReplayRunner:
    """
    Feeds an iterable of Nautilus-compatible objects through the adapter into a sink.

    Does not own a thread or a timer — callers drive iteration by calling run_bars /
    run_trades.  This keeps the runner easy to test and easy to compose with any
    scheduling strategy (background thread, asyncio, step-by-step, etc.).
    """

    def __init__(self, sink: ReplayEventSink) -> None:
        self._sink = sink

    def run_bars(self, bars: Iterable) -> None:
        """Push each bar as ReplayTimeUpdated → KlineUpdate."""
        for bar in bars:
            self._sink.apply_replay_event(timestamp_ns_to_replay_time_updated(bar.ts_event))
            self._sink.apply_replay_event(bar_to_kline_update(bar))

    def run_trades(self, trades: Iterable) -> None:
        """Push each trade as ReplayTimeUpdated → TradeUpdate."""
        for trade in trades:
            self._sink.apply_replay_event(timestamp_ns_to_replay_time_updated(trade.ts_event))
            self._sink.apply_replay_event(trade_to_trade_update(trade))
