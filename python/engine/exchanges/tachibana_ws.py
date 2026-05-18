"""Tachibana EVENT WebSocket helpers (Phase 8 §3.2 A3.1).

This module currently exposes:

* :func:`is_market_open` — pure JST market-hours check (東証 前場/後場/クロージング)
* :class:`FdFrameProcessor` — stateful per-row FD frame → trade + depth synthesis

The async WS connection manager (``TachibanaEventWs``) and the multiplexer hub
(``TickerEventWsHub``) live in the upstream e-station codebase and will be
ported in a later subtask once the codec + processor have GREEN coverage here.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import datetime, time as dtime, timedelta, timezone
from decimal import Decimal, InvalidOperation
from typing import Any

JST = timezone(timedelta(hours=9))
log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Market hours (Tokyo Stock Exchange, effective 2024-11-05)
# ---------------------------------------------------------------------------

# 前場  09:00–11:30
# 昼休  11:30–12:30
# 後場  12:30–15:25  (regular)
# クロージング・オークション  15:25–15:30
# → WS connection kept alive until 15:30; closed only from 15:30 onward.
_SESSION_WINDOWS: tuple[tuple[dtime, dtime], ...] = (
    (dtime(9, 0), dtime(11, 30)),
    (dtime(12, 30), dtime(15, 30)),  # 後場 + クロージング合算
)


def is_market_open(now_jst: datetime) -> bool:
    """Return True if ``now_jst`` falls within any Tokyo trading session.

    Naive datetimes are treated as UTC, matching the convention in
    ``tachibana.py::current_jst_yyyymmdd``.  Holiday calendars are
    intentionally out of scope (Phase 1 design decision).
    """
    if now_jst.tzinfo is None:
        now_jst = now_jst.replace(tzinfo=timezone.utc)
    t = now_jst.astimezone(JST).time().replace(tzinfo=None)
    return any(start <= t < end for start, end in _SESSION_WINDOWS)


# ---------------------------------------------------------------------------
# FD frame processor — stateful, per-row
# ---------------------------------------------------------------------------


@dataclass
class FdFrameProcessor:
    """Convert FD (time-and-sales) event frames into trade + depth dicts.

    Designed for use with one ``p_gyou_no`` row per instance. The caller
    must call :meth:`reset` when the underlying WebSocket reconnects or the
    subscribed ticker changes, to avoid carrying stale DV/quote state
    across session boundaries (F4).

    :meth:`process` returns ``(trade_dict | None, depth_dict | None)``.
    ``trade_dict`` is omitted on the first frame and when DV does not
    increase. ``depth_dict`` is omitted when no bid/ask keys are present.
    """

    row: str

    _prev_dv: Decimal | None = field(default=None, init=False, repr=False)
    _prev_bid: Decimal | None = field(default=None, init=False, repr=False)
    _prev_ask: Decimal | None = field(default=None, init=False, repr=False)
    _prev_trade_price: Decimal | None = field(default=None, init=False, repr=False)
    _sequence_id: int = field(default=0, init=False, repr=False)

    def reset(self) -> None:
        """Reset DV/quote/sequence state (call on reconnect or ticker change)."""
        self._prev_dv = None
        self._prev_bid = None
        self._prev_ask = None
        self._prev_trade_price = None
        self._sequence_id = 0

    def process(
        self, fields: dict[str, str], recv_ts_ms: int
    ) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
        """Process one FD frame.

        Args:
            fields:      ``(key, value)`` pairs from ``parse_event_frame``,
                         converted to a flat dict by the caller.
            recv_ts_ms:  Unix-millisecond receive timestamp (fallback for ts_ms).

        Returns:
            ``(trade | None, depth | None)``
        """
        row = self.row
        dpp_str = fields.get(f"p_{row}_DPP", "")
        dv_str = fields.get(f"p_{row}_DV", "")

        if not dpp_str or not dv_str:
            return None, None

        try:
            dpp = Decimal(dpp_str)
            dv = Decimal(dv_str)
        except InvalidOperation:
            log.warning(
                "tachibana: FdFrameProcessor.process: InvalidOperation for row=%s fields_keys=%s",
                self.row, list(fields.keys())[:5],
            )
            return None, None

        depth = self._extract_depth(fields, recv_ts_ms)
        trade: dict[str, Any] | None = None

        if self._prev_dv is None:
            # First frame: initialize state, no trade (F4).
            self._prev_dv = dv
            self._prev_bid = self._extract_best_bid(fields)
            self._prev_ask = self._extract_best_ask(fields)
        elif dv < self._prev_dv:
            # DV reset (session rollover / new day): reinitialize (F4).
            log.debug(
                "tachibana ws: DV reset row=%s prev=%s curr=%s; reinitializing",
                row, self._prev_dv, dv,
            )
            self._prev_dv = dv
            self._prev_bid = self._extract_best_bid(fields)
            self._prev_ask = self._extract_best_ask(fields)
        else:
            qty = dv - self._prev_dv
            if qty > 0:
                _side = self._determine_side(dpp)
                ts_ms = self._parse_ts_ms(fields, recv_ts_ms, row)
                trade = {
                    "price": str(dpp),
                    "qty": str(qty),
                    "side": _side if _side is not None else "unknown",
                    "ts_ms": ts_ms,
                    "is_liquidation": False,
                }
                self._prev_trade_price = dpp

            # Update quote after trade synthesis (quote rule: use prev frame's quote).
            self._prev_dv = dv
            self._prev_bid = self._extract_best_bid(fields)
            self._prev_ask = self._extract_best_ask(fields)

        return trade, depth

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    def _determine_side(self, price: Decimal) -> str | None:
        """Quote rule + tick rule (F3, data-mapping §3). Returns None when ambiguous."""
        if self._prev_ask is not None and price >= self._prev_ask:
            return "buy"
        if self._prev_bid is not None and price <= self._prev_bid:
            return "sell"
        # Midpoint: tick rule
        if self._prev_trade_price is not None:
            if price > self._prev_trade_price:
                return "buy"
            if price < self._prev_trade_price:
                return "sell"
        # Ambiguous (F-M8b)
        log.warning("tachibana ws: trade side ambiguous for price %s", price)
        return None

    def _extract_best_bid(self, fields: dict[str, str]) -> Decimal | None:
        v = fields.get(f"p_{self.row}_GBP1", "")
        try:
            return Decimal(v) if v else None
        except InvalidOperation:
            return None

    def _extract_best_ask(self, fields: dict[str, str]) -> Decimal | None:
        v = fields.get(f"p_{self.row}_GAP1", "")
        try:
            return Decimal(v) if v else None
        except InvalidOperation:
            return None

    def _extract_depth(
        self, fields: dict[str, str], recv_ts_ms: int
    ) -> dict[str, Any] | None:
        row = self.row
        bids: list[dict[str, str]] = []
        asks: list[dict[str, str]] = []
        for i in range(1, 11):
            bp = fields.get(f"p_{row}_GBP{i}", "")
            bv = fields.get(f"p_{row}_GBV{i}", "")
            ap = fields.get(f"p_{row}_GAP{i}", "")
            av = fields.get(f"p_{row}_GAV{i}", "")
            if bp:
                bids.append({"price": bp, "qty": bv})
            if ap:
                asks.append({"price": ap, "qty": av})

        if not bids and not asks:
            return None

        self._sequence_id += 1
        return {
            "bids": bids,
            "asks": asks,
            "sequence_id": self._sequence_id,
            "recv_ts_ms": recv_ts_ms,
        }

    @staticmethod
    def _parse_ts_ms(fields: dict[str, str], fallback_ms: int, row: str) -> int:
        """ts_ms priority: DPP:T > p_date > recv fallback (data-mapping §3 F17)."""
        p_date = fields.get("p_date", "")
        if p_date:
            # Format: YYYY.MM.DD-HH:MM:SS.TTT  (T = tenths/hundredths/ms)
            try:
                dt = datetime.strptime(p_date, "%Y.%m.%d-%H:%M:%S.%f")
                dt_jst = dt.replace(tzinfo=JST)
                return int(dt_jst.timestamp() * 1000)
            except ValueError:
                pass
        dpp_t = fields.get(f"p_{row}_DPP:T", "")
        if dpp_t:
            # Format: HH:MM — combine with today's JST date.
            try:
                now_jst = datetime.now(JST)
                t = datetime.strptime(dpp_t, "%H:%M")
                dt_jst = now_jst.replace(
                    hour=t.hour, minute=t.minute, second=0, microsecond=0
                )
                return int(dt_jst.timestamp() * 1000)
            except ValueError:
                pass
        return fallback_ms


__all__ = [
    "FdFrameProcessor",
    "is_market_open",
]
