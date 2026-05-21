"""Strategy → UI log channel (Phase 10 §570, adapted to this Nautilus version).

§570 originally assumed the backend could transparently tap every `self.log.*`
call and relay it to the Live Run Panel. This Nautilus version makes that
impossible: `Logger.{info,warning,error}` route straight into the **Rust** logging
subsystem (`nautilus_pyo3.logger_log`), `init_logging` only supports stdout/file
output (no Python sink), the strategy's `_log` is a `cdef readonly` attribute, and
`Logger` is a non-monkeypatchable extension type.

So instead of auto-tapping, a strategy surfaces a UI-facing log line by calling
`emit_strategy_log(self, message, level)`, which:

1. mirrors the line to Nautilus's structured log (`self.log.<level>`), so it still
   reaches stdout/file exactly like a normal `self.log.*` call; and
2. publishes a `StrategyLogRecord` on the msgbus topic `strategy.log.{strategy_id}`.

The controller subscribes to that topic (mirroring the `events.order.{strategy_id}`
bridge in `engine_controller.py`) and forwards each record to the UI as a
`StrategyLogMessage` BackendEvent. This reuses the proven, non-blocking msgbus seam
— no global-logging reconfiguration, no `*.log` litter, and it honours the live-loop
deadlock invariant (handlers only do `msgbus.send`-style work).

Tradeoff (unavoidable, from Nautilus having no Python log sink): plain `self.log.*`
lines are NOT auto-relayed — only lines emitted via this helper reach the panel.
"""

from __future__ import annotations

from dataclasses import dataclass

# topic prefix; subscribers/publishers compose the per-run topic with the
# (forced) nautilus strategy id, e.g. "strategy.log.LIVE-abcd1234".
STRATEGY_LOG_TOPIC_PREFIX = "strategy.log"

# UI-facing level → Logger method name. Unknown levels normalise to INFO so a
# typo in strategy code can never crash the run.
_LEVEL_METHODS = {
    "DEBUG": "debug",
    "INFO": "info",
    "WARNING": "warning",
    "ERROR": "error",
}


def strategy_log_topic(strategy_id: str) -> str:
    """The msgbus topic carrying one run's UI log lines."""
    return f"{STRATEGY_LOG_TOPIC_PREFIX}.{strategy_id}"


@dataclass(frozen=True)
class StrategyLogRecord:
    """One UI-facing strategy log line published on `strategy.log.{strategy_id}`."""

    level: str
    message: str
    ts_ns: int


def emit_strategy_log(strategy, message, level: str = "INFO") -> None:
    """Surface a UI log line in the Live Run Panel (and Nautilus's normal log).

    Call from inside a `Strategy` after `on_start` (the msgbus/clock are injected at
    registration). Safe to call from any strategy callback.

    Parameters
    ----------
    strategy : nautilus_trader.trading.strategy.Strategy
        The calling strategy (`self`).
    message : str
        The log line text.
    level : str, default "INFO"
        One of DEBUG / INFO / WARNING / ERROR (case-insensitive); unknown → INFO.
    """
    message = str(message)
    level = (level or "INFO").upper()
    if level not in _LEVEL_METHODS:
        level = "INFO"
    # (1) mirror to Nautilus's structured log (stdout/file), like a normal call.
    getattr(strategy.log, _LEVEL_METHODS[level])(message)
    # (2) publish for the UI relay. best-effort: a logging call must never crash
    # the strategy (e.g. if called before registration wires the msgbus).
    try:
        strategy.msgbus.publish(
            strategy_log_topic(str(strategy.id)),
            StrategyLogRecord(
                level=level,
                message=message,
                ts_ns=strategy.clock.timestamp_ns(),
            ),
        )
    except Exception:  # noqa: BLE001 — logging is best-effort, never fatal.
        pass
