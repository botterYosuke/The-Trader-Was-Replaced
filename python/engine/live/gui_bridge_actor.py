"""GuiBridgeActor — bridges BacktestEngine bar events to RustBacktestSink (issue #68 Slice 1).

Slice 1: bars only.  Slice 2 adds Pause/Step threading.Event control.
"""
from __future__ import annotations

import json
import logging
from typing import Any

log = logging.getLogger(__name__)


class GuiBridgeActor:
    """Accumulates OHLC state from BacktestEngine bar callbacks and pushes state JSON to Rust.

    Uses BacktestEngine msgbus subscription rather than nautilus Actor subclassing.
    Keeps a running list of ohlc_points / history and serialises a minimal
    BackendTradingState-compatible JSON on every bar via rust_sink.push_bar().
    """

    def __init__(self, rust_sink: Any, instrument_id: str = "") -> None:
        self._sink = rust_sink
        self._instrument_id = instrument_id
        self._ohlc_points: list[dict] = []
        self._history: list[float] = []

    def make_bar_handler(self):
        """Return a callable suitable for engine.kernel.msgbus.subscribe(handler=...)."""

        def _on_bar(bar) -> None:
            try:
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
