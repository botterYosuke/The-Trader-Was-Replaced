"""GuiBridgeActor — bridges BacktestEngine bar events to RustBacktestSink (issue #68).

Slice 1: bars only.
Slice 2: pause_event / step_event threading.Event control for Pause/Step/Resume.
Slice 3: make_order_handler() — OrderFilled events → sink.push_order().
Slice 4: make_position_handler() — PositionOpened/Changed events → sink.push_portfolio().
Slice 7: speed_ref — list[float] mutable cell; bar handler sleeps to simulate replay speed.
"""
from __future__ import annotations

import json
import logging
import time
from typing import Any, Optional

log = logging.getLogger(__name__)

BASE_DELAY_S = 0.1  # seconds per bar at speed_ref[0] == 1.0


class GuiBridgeActor:
    """Accumulates OHLC state from BacktestEngine bar callbacks and pushes state JSON to Rust.

    Uses BacktestEngine msgbus subscription rather than nautilus Actor subclassing.
    Keeps a running list of ohlc_points / history and serialises a minimal
    BackendTradingState-compatible JSON on every bar via rust_sink.push_bar().

    Slice 2 additions:
      pause_event — threading.Event; set=running, clear=paused.
                    If None, bars are always processed (backward-compatible).
      step_event  — threading.Event; set=allow one bar through while paused.
                    Consumed (cleared) after each single-step bar.

    Slice 7 additions:
      speed_ref   — list[float] with one element (mutable cell).
                    If not None, bar handler calls time.sleep(BASE_DELAY_S / speed_ref[0])
                    after each bar to simulate replay speed.
                    If None (default), no sleep is inserted.
    """

    def __init__(
        self,
        rust_sink: Any,
        instrument_id: str = "",
        *,
        pause_event: Optional[Any] = None,
        step_event: Optional[Any] = None,
        speed_ref: Optional[list] = None,
    ) -> None:
        self._sink = rust_sink
        self._instrument_id = instrument_id
        self._ohlc_points: list[dict] = []
        self._history: list[float] = []
        self._pause_event = pause_event
        self._step_event = step_event
        self._speed_ref = speed_ref

    def make_bar_handler(self):
        """Return a callable suitable for engine.kernel.msgbus.subscribe(handler=...)."""

        def _on_bar(bar) -> None:
            try:
                # --- Pause/Step gate (Slice 2) ---
                if self._pause_event is not None:
                    while not self._pause_event.is_set():
                        if self._step_event is not None and self._step_event.is_set():
                            self._step_event.clear()
                            break
                        self._pause_event.wait(timeout=0.02)

                ts_ms = bar.ts_event // 1_000_000
                o = float(bar.open.as_double())
                h = float(bar.high.as_double())
                l = float(bar.low.as_double())
                c = float(bar.close.as_double())
                v = float(bar.volume.as_double())

                self._ohlc_points.append(
                    {
                        "timestamp_ms": ts_ms,
                        "open_time_ms": ts_ms,
                        "open": o,
                        "high": h,
                        "low": l,
                        "close": c,
                        "volume": v,
                    }
                )
                self._history.append(c)

                self._sink.push_bar(
                    json.dumps(
                        {
                            "price": c,
                            "timestamp": ts_ms / 1000.0,
                            "timestamp_ms": ts_ms,
                            "history": self._history,
                            "ohlc_points": self._ohlc_points,
                        }
                    )
                )

                # --- Speed delay (Slice 7) ---
                if self._speed_ref is not None and self._speed_ref[0] > 0:
                    time.sleep(BASE_DELAY_S / self._speed_ref[0])

            except Exception:
                log.warning("[GuiBridgeActor] on_bar failed", exc_info=True)

        return _on_bar

    def make_order_handler(self):
        """Return a callable for OrderFilled / order events (Slice 3).

        Duck-typing: only events with last_qty attribute (OrderFilled) are forwarded.
        """

        def _on_order(event) -> None:
            try:
                if not hasattr(event, "last_qty"):
                    return

                ts_ms = event.ts_event // 1_000_000
                payload = {
                    "symbol": str(event.instrument_id),
                    "client_order_id": str(event.client_order_id),
                    "venue_order_id": str(event.venue_order_id),
                    "strategy_id": str(event.strategy_id),
                    "side": event.order_side.name,
                    "status": "FILLED",
                    "qty": float(event.last_qty.as_double()),
                    "price": float(event.last_px.as_double()),
                    "timestamp_ms": ts_ms,
                }
                self._sink.push_order(json.dumps(payload))
            except Exception:
                log.warning("[GuiBridgeActor] on_order failed", exc_info=True)

        return _on_order

    def make_position_handler(self, cache: Any, venue_str: str):
        """Return a callable for PositionOpened / PositionChanged events (Slice 4).

        If cache is None (test mode), returns zero-valued portfolio snapshot.
        """

        def _on_position(event) -> None:
            try:
                if cache is None:
                    buying_power = 0.0
                    equity = 0.0
                    positions: list[dict] = []
                else:
                    try:
                        account = cache.account_for_venue(venue_str)
                        buying_power = float(account.balance_free().as_double()) if account else 0.0
                        equity = float(account.balance_total().as_double()) if account else 0.0
                    except Exception:
                        buying_power = 0.0
                        equity = 0.0

                    raw_positions = cache.positions() if hasattr(cache, "positions") else []
                    positions = [
                        {
                            "symbol": str(p.instrument_id),
                            "qty": float(p.quantity.as_double()),
                            "avg_price": float(p.avg_px_open) if p.avg_px_open else 0.0,
                        }
                        for p in (raw_positions or [])
                    ]

                payload = {
                    "buying_power": buying_power,
                    "equity": equity,
                    "positions": positions,
                    "orders": [],
                }
                self._sink.push_portfolio(json.dumps(payload))
            except Exception:
                log.warning("[GuiBridgeActor] on_position failed", exc_info=True)

        return _on_position
