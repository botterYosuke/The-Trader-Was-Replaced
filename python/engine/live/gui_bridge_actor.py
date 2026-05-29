"""GuiBridgeActor — bridges BacktestEngine bar events to RustBacktestSink (issue #68).

Slice 1: bars only.
Slice 2: pause_event / step_event threading.Event control for Pause/Step/Resume.
"""
from __future__ import annotations

import json
import logging
from typing import Any, Optional

log = logging.getLogger(__name__)


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
    """

    def __init__(
        self,
        rust_sink: Any,
        instrument_id: str = "",
        *,
        pause_event: Optional[Any] = None,
        step_event: Optional[Any] = None,
    ) -> None:
        self._sink = rust_sink
        self._instrument_id = instrument_id
        self._ohlc_points: list[dict] = []
        self._history: list[float] = []
        self._pause_event = pause_event
        self._step_event = step_event

    def make_bar_handler(self):
        """Return a callable suitable for engine.kernel.msgbus.subscribe(handler=...)."""

        def _on_bar(bar) -> None:
            try:
                # --- Pause/Step gate (Slice 2) ---
                # threading.Event.wait() releases the GIL, allowing the Python
                # worker thread to process Pause/Step/Resume commands concurrently.
                if self._pause_event is not None:
                    while not self._pause_event.is_set():
                        # Single-step: allow one bar through without full resume
                        if self._step_event is not None and self._step_event.is_set():
                            self._step_event.clear()  # consume the step token
                            break
                        # Block briefly, releasing GIL so worker thread can run
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
            except Exception:
                log.warning("[GuiBridgeActor] on_bar failed", exc_info=True)

        return _on_bar
