from dataclasses import dataclass, field
from typing import Union

from .models import HistoryPoint, OhlcPoint


@dataclass(frozen=True)
class ReplayTimeUpdated:
    timestamp_ms: int


@dataclass(frozen=True)
class KlineUpdate:
    timestamp_ms: int
    close: float
    open: float = 0.0
    high: float = 0.0
    low: float = 0.0
    open_time_ms: int = 0


@dataclass(frozen=True)
class TradeUpdate:
    timestamp_ms: int
    price: float


ReplayEvent = Union[ReplayTimeUpdated, KlineUpdate, TradeUpdate]


@dataclass
class ReducerState:
    timestamp_ms: int
    price: float
    open: float = 0.0
    high: float = 0.0
    low: float = 0.0
    open_time_ms: int = 0
    history: list = field(default_factory=list)
    history_points: list = field(default_factory=list)
    ohlc_points: list = field(default_factory=list)
    max_history_len: int = 1000


def apply_event(state: ReducerState, event: ReplayEvent) -> None:
    """Apply event to state in place. Stale timestamps are silently ignored."""
    if isinstance(event, ReplayTimeUpdated):
        if event.timestamp_ms >= state.timestamp_ms:
            state.timestamp_ms = event.timestamp_ms
        return

    if isinstance(event, (KlineUpdate, TradeUpdate)):
        ts = event.timestamp_ms
        if ts < state.timestamp_ms:
            return

        price = event.close if isinstance(event, KlineUpdate) else event.price
        state.timestamp_ms = ts
        state.price = price
        if isinstance(event, KlineUpdate):
            state.open = event.open
            state.high = event.high
            state.low = event.low
            state.open_time_ms = event.open_time_ms
            ohlc_open_time = event.open_time_ms if event.open_time_ms > 0 else ts
            state.ohlc_points.append(OhlcPoint(
                timestamp_ms=ts,
                open_time_ms=ohlc_open_time,
                open=event.open if event.open > 0 else price,
                high=event.high if event.high > 0 else price,
                low=event.low if event.low > 0 else price,
                close=price,
            ))
            if len(state.ohlc_points) > state.max_history_len:
                state.ohlc_points.pop(0)
        state.history.append(price)
        state.history_points.append(HistoryPoint(timestamp_ms=ts, price=price))

        if len(state.history) > state.max_history_len:
            state.history.pop(0)
            state.history_points.pop(0)
