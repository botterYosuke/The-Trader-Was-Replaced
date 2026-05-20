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
    instrument_id: str = ""  # D9: multi-instrument support (existing calls use "" default)
    volume: float = 0.0


@dataclass(frozen=True)
class TradeUpdate:
    timestamp_ms: int
    price: float
    instrument_id: str = ""  # D9: multi-instrument support (existing calls use "" default)


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
    per_id_close: dict = field(default_factory=dict)  # D9: {instrument_id -> last close}
    per_id_ohlc_points: dict = field(default_factory=dict)  # A0: {instrument_id -> list[OhlcPoint]}


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

        # D9: per-id close tracking for multi-instrument sidebar last prices
        iid = getattr(event, "instrument_id", "")
        if iid:
            state.per_id_close[iid] = price

        # Primary instrument: update price/history/ohlc only if no instrument_id
        # (primary/legacy path) or if instrument_id is empty (backward compat)
        is_primary = not iid

        if is_primary:
            state.timestamp_ms = ts
            state.price = price
        elif ts >= state.timestamp_ms:
            state.timestamp_ms = ts
            state.price = price

        if isinstance(event, KlineUpdate):
            if is_primary:
                state.open = event.open
                state.high = event.high
                state.low = event.low
                state.open_time_ms = event.open_time_ms
            ohlc_open_time = event.open_time_ms if event.open_time_ms > 0 else ts
            if is_primary:
                state.ohlc_points.append(OhlcPoint(
                    timestamp_ms=ts,
                    open_time_ms=ohlc_open_time,
                    open=event.open if event.open > 0 else price,
                    high=event.high if event.high > 0 else price,
                    low=event.low if event.low > 0 else price,
                    close=price,
                    volume=event.volume,
                ))
                if len(state.ohlc_points) > state.max_history_len:
                    state.ohlc_points.pop(0)
            # A0: per-id OHLC accumulation (fires on every real-id KlineUpdate; primary double-emits its real id too)
            if iid:
                pts = state.per_id_ohlc_points.setdefault(iid, [])
                pts.append(OhlcPoint(
                    timestamp_ms=ts,
                    open_time_ms=ohlc_open_time,
                    open=event.open if event.open > 0 else price,
                    high=event.high if event.high > 0 else price,
                    low=event.low if event.low > 0 else price,
                    close=price,
                    volume=event.volume,
                ))
                if len(pts) > state.max_history_len:
                    pts.pop(0)
        if is_primary:
            state.history.append(price)
            state.history_points.append(HistoryPoint(timestamp_ms=ts, price=price))

            if len(state.history) > state.max_history_len:
                state.history.pop(0)
                state.history_points.pop(0)
