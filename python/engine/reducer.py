from dataclasses import dataclass, field
from typing import Union

from .models import HistoryPoint


@dataclass(frozen=True)
class ReplayTimeUpdated:
    timestamp_ms: int


@dataclass(frozen=True)
class KlineUpdate:
    timestamp_ms: int
    close: float


@dataclass(frozen=True)
class TradeUpdate:
    timestamp_ms: int
    price: float


ReplayEvent = Union[ReplayTimeUpdated, KlineUpdate, TradeUpdate]


@dataclass
class ReducerState:
    timestamp_ms: int
    price: float
    history: list = field(default_factory=list)
    history_points: list = field(default_factory=list)
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
        state.history.append(price)
        state.history_points.append(HistoryPoint(timestamp_ms=ts, price=price))

        if len(state.history) > state.max_history_len:
            state.history.pop(0)
            state.history_points.pop(0)
