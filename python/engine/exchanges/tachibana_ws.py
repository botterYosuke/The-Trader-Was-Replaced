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


# ---------------------------------------------------------------------------
# TachibanaEventWs — async WebSocket connection manager (Phase 8 §3.2 A3.2a)
# ---------------------------------------------------------------------------

import asyncio
from collections import Counter

# websockets is an optional dependency at import time so that unit tests
# that only exercise FdFrameProcessor can run without it.
try:
    import websockets  # type: ignore[import-untyped]
    from websockets.exceptions import ConnectionClosed  # type: ignore[import-untyped]
    _HAS_WEBSOCKETS = True
except ImportError:  # pragma: no cover
    websockets = None  # type: ignore[assignment]
    ConnectionClosed = Exception  # type: ignore[assignment,misc]
    _HAS_WEBSOCKETS = False

# How long to wait for any frame (KP or data) before treating the connection
# as dead.  12 s = KP_INTERVAL(5) * 2 + 2 s jitter (plan §T5 M2 修正).
_DEAD_FRAME_TIMEOUT_S: float = 12.0

# Exponential back-off for reconnects: [1, 2, 4, 8, 16, 30] seconds.
_BACKOFF_CAPS: tuple[float, ...] = (1.0, 2.0, 4.0, 8.0, 16.0, 30.0)

# Interval between frame-count stat log lines (§6 O1). Module-level so tests can patch.
_FRAME_STATS_INTERVAL_S: float = 25.0


class TachibanaEventWs:
    """Async WS connection manager that dispatches parsed EVENT frames.

    Usage (inside stream_trades / stream_depth)::

        ws = TachibanaEventWs(url, stop_event, ticker="7203")
        await ws.run(callback)

    The loop handles reconnects internally with exponential back-off and a
    dead-frame watchdog (no frame within ``_DEAD_FRAME_TIMEOUT_S`` → tear down
    and reconnect). It exits cleanly when ``stop_event`` is set.

    ``stop_event`` must be an ``asyncio.Event`` that the caller sets to
    request graceful shutdown.
    """

    def __init__(
        self,
        url: str,
        stop_event: asyncio.Event,
        *,
        ticker: str,
        venue: str = "tachibana",
        proxy: str | None = None,
    ) -> None:
        from .tachibana_codec import decode_response_body, parse_event_frame

        self._url = url
        self._stop = stop_event
        self._ticker = ticker
        self._venue = venue
        self._proxy = proxy
        self._decode = decode_response_body
        self._parse = parse_event_frame
        self._conn_count = 0

    async def run(
        self,
        callback: Any,
        *,
        on_connect: Any | None = None,
    ) -> None:
        """Drive the WS loop, calling ``callback(frame_type, fields, recv_ts_ms)``
        for each received frame.  Returns when ``stop_event`` is set.

        ``on_connect`` is an optional zero-argument callable invoked at the start
        of each connection attempt (before the handshake), including reconnects.
        Use it to reset per-connection state (e.g. rate-limit dicts).
        """
        if not _HAS_WEBSOCKETS:
            raise RuntimeError(
                "tachibana_ws.TachibanaEventWs requires the 'websockets' package"
            )
        backoff_idx = 0
        while not self._stop.is_set():
            self._conn_count += 1
            if on_connect is not None:
                on_connect()
            try:
                await self._connect_once(callback)
                backoff_idx = 0
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                if self._stop.is_set():
                    return
                backoff = _BACKOFF_CAPS[min(backoff_idx, len(_BACKOFF_CAPS) - 1)]
                backoff_idx += 1
                log.warning(
                    "tachibana ws: %s disconnected (%s); reconnecting in %.2f s",
                    self._ticker, exc, backoff,
                )
                try:
                    await asyncio.wait_for(
                        self._stop.wait(), timeout=backoff
                    )
                except asyncio.TimeoutError:
                    pass

    async def _connect_once(self, callback: Any) -> None:
        connect_kwargs: dict[str, Any] = {"ping_interval": None}
        if self._proxy is not None:
            connect_kwargs["proxy"] = self._proxy
        async with websockets.connect(self._url, **connect_kwargs) as ws:
            log.info(
                "tachibana ws: connected ticker=%s conn=#%d",
                self._ticker, self._conn_count,
            )

            loop = asyncio.get_event_loop()
            last_frame_t: list[float] = [loop.time()]
            dead_event = asyncio.Event()
            # Frame counters for observability (§6 O1). Counter() avoids
            # KeyError for unknown evt_cmd values.
            _frame_counts: Counter[str] = Counter()
            _last_stats_t: list[float] = [loop.time()]

            async def _recv_loop() -> None:
                async for raw in ws:
                    last_frame_t[0] = loop.time()
                    recv_ts_ms = int(datetime.now(timezone.utc).timestamp() * 1000)
                    # Shift-JIS decode for bytes payload; pass str through decode_response_body.
                    if isinstance(raw, bytes):
                        text = self._decode(raw)
                    else:
                        text = raw

                    pairs = self._parse(text)
                    fields: dict[str, str] = {k: v for k, v in pairs}

                    evt_cmd = fields.get("p_cmd", "")
                    if evt_cmd == "KP":
                        _frame_counts["KP"] += 1
                        log.debug("tachibana ws: KP recv %s", self._ticker)
                        await callback("KP", fields, recv_ts_ms)
                    elif evt_cmd == "FD":
                        _frame_counts["FD"] += 1
                        await callback("FD", fields, recv_ts_ms)
                    elif evt_cmd == "ST":
                        _frame_counts["ST"] += 1
                        p_errno = fields.get("p_errno", "?")
                        log.warning(
                            "tachibana ws: ST frame ticker=%s p_errno=%s (total ST=%d)",
                            self._ticker, p_errno, _frame_counts["ST"],
                        )
                        await callback("ST", fields, recv_ts_ms)
                    else:
                        _frame_counts["other"] += 1
                        log.debug(
                            "tachibana ws: unknown evt_cmd=%r ticker=%s",
                            evt_cmd, self._ticker,
                        )

                    now = loop.time()
                    if now - _last_stats_t[0] >= _FRAME_STATS_INTERVAL_S:
                        _last_stats_t[0] = now
                        log.info(
                            "tachibana ws: frame stats ticker=%s "
                            "FD=%d KP=%d ST=%d other=%d (conn #%d cumulative)",
                            self._ticker,
                            _frame_counts["FD"], _frame_counts["KP"],
                            _frame_counts["ST"], _frame_counts["other"],
                            self._conn_count,
                        )

                    if self._stop.is_set():
                        return

            async def _watchdog() -> None:
                # Adaptive check interval: at most 1s, at most half the timeout.
                interval = min(1.0, _DEAD_FRAME_TIMEOUT_S / 2.0)
                while not self._stop.is_set():
                    await asyncio.sleep(interval)
                    elapsed = loop.time() - last_frame_t[0]
                    if elapsed >= _DEAD_FRAME_TIMEOUT_S:
                        log.warning(
                            "tachibana ws: %s dead-frame timeout (%.1f s); reconnecting. "
                            "Frame counts: FD=%d KP=%d ST=%d other=%d",
                            self._ticker, elapsed,
                            _frame_counts["FD"], _frame_counts["KP"],
                            _frame_counts["ST"], _frame_counts["other"],
                        )
                        dead_event.set()
                        return

            recv_task = asyncio.create_task(_recv_loop())
            watchdog_task = asyncio.create_task(_watchdog())
            stop_task = asyncio.create_task(self._stop.wait())

            done, pending = await asyncio.wait(
                [recv_task, watchdog_task, stop_task],
                return_when=asyncio.FIRST_COMPLETED,
            )
            for t in pending:
                t.cancel()
                try:
                    await t
                except (asyncio.CancelledError, Exception):
                    pass

            if dead_event.is_set():
                raise ConnectionError("dead-frame timeout")

            # Re-raise any unhandled exception from the recv loop.
            if recv_task in done:
                exc = recv_task.exception()
                if exc is not None:
                    raise exc


# ---------------------------------------------------------------------------
# Per-ticker EVENT WS multiplexer (Phase 8 §3.2 A3.2b)
# ---------------------------------------------------------------------------


class TickerEventWsHub:
    """ticker 毎に EVENT WS を 1 本だけ張り、frame を複数 subscriber に fanout する。

    立花 EVENT WS は ``(session, p_issue_code)`` 単位で 1 接続のみ許容する。
    ``stream_depth`` と ``stream_trades`` がそれぞれ独立に WS を張ると broker が
    片側を ``p_errno=2 'session inactive.'`` で蹴る (Bug Y / 2026-05-04 観測)。

    Hub は単一の :class:`TachibanaEventWs` を所有し、最初の :meth:`subscribe`
    で WS タスクを起動、最後の :meth:`unsubscribe` で停止する。
    フレームは登録順に subscriber へ ``await`` で配り、1 subscriber の例外は
    他 subscriber に伝播させない (log のみ)。
    """

    def __init__(
        self,
        ws_url: str,
        *,
        ticker: str,
        proxy: str | None = None,
    ) -> None:
        self._ws_url = ws_url
        self._ticker = ticker
        self._proxy = proxy
        self._subscribers: dict[str, Any] = {}
        self._on_connect_cbs: dict[str, Any] = {}
        self._on_close_cbs: dict[str, Any] = {}
        self._stop_event: asyncio.Event = asyncio.Event()
        self._runner_task: asyncio.Task | None = None
        self._lock: asyncio.Lock = asyncio.Lock()

    @property
    def subscriber_count(self) -> int:
        return len(self._subscribers)

    async def subscribe(
        self, key: str, callback: Any, *,
        on_connect: Any | None = None,
        on_close: Any | None = None,
    ) -> None:
        """``callback(frame_type, fields, recv_ts_ms)`` を登録する。

        同じ key の二重 subscribe は警告ログのみで no-op。
        最初の subscriber 登録時に WS タスクを起動する。
        """
        async with self._lock:
            if key in self._subscribers:
                log.warning(
                    "TickerEventWsHub[%s]: duplicate subscribe key=%r ignored",
                    self._ticker, key,
                )
                return
            self._subscribers[key] = callback
            if on_connect is not None:
                self._on_connect_cbs[key] = on_connect
            if on_close is not None:
                self._on_close_cbs[key] = on_close
            if self._runner_task is None or self._runner_task.done():
                self._stop_event = asyncio.Event()
                self._runner_task = asyncio.create_task(self._run())

    async def unsubscribe(self, key: str) -> None:
        """``key`` の subscriber を外す。最後の 1 つが外れたら WS タスクを停止する。

        存在しない key は no-op。on_close は呼ばない (自発的な離脱のため)。
        """
        async with self._lock:
            self._subscribers.pop(key, None)
            self._on_connect_cbs.pop(key, None)
            self._on_close_cbs.pop(key, None)
            if not self._subscribers and self._runner_task is not None:
                self._stop_event.set()

    async def aclose(self) -> None:
        """全 subscriber を破棄して WS タスクを止める (session swap などで使う)。

        破棄前に各 subscriber の ``on_close`` を呼ぶ。
        """
        async with self._lock:
            close_cbs = list(self._on_close_cbs.items())
            self._subscribers.clear()
            self._on_connect_cbs.clear()
            self._on_close_cbs.clear()
            self._stop_event.set()
            task = self._runner_task
        for key, cb in close_cbs:
            try:
                cb()
            except Exception:
                log.exception(
                    "TickerEventWsHub[%s]: on_close for %r raised",
                    self._ticker, key,
                )
        if task is not None and not task.done():
            try:
                await asyncio.wait_for(task, timeout=2.0)
            except asyncio.TimeoutError:
                task.cancel()
                try:
                    await task
                except (asyncio.CancelledError, Exception):
                    pass

    async def _run(self) -> None:
        ws = TachibanaEventWs(
            self._ws_url, self._stop_event,
            ticker=self._ticker, proxy=self._proxy,
        )
        try:
            await ws.run(self._dispatch, on_connect=self._on_ws_connect)
        except Exception:
            log.exception(
                "TickerEventWsHub[%s]: WS run loop raised", self._ticker,
            )

    def _on_ws_connect(self) -> None:
        """WS (再)接続毎に全 subscriber の on_connect を順に発火する。"""
        for key, cb in list(self._on_connect_cbs.items()):
            try:
                cb()
            except Exception:
                log.exception(
                    "TickerEventWsHub[%s]: on_connect for %r raised",
                    self._ticker, key,
                )

    async def _dispatch(
        self, frame_type: str, fields: dict[str, str], recv_ts_ms: int,
    ) -> None:
        for key, cb in list(self._subscribers.items()):
            try:
                await cb(frame_type, fields, recv_ts_ms)
            except Exception:
                log.exception(
                    "TickerEventWsHub[%s]: subscriber %r raised on %s frame",
                    self._ticker, key, frame_type,
                )


__all__ = [
    "FdFrameProcessor",
    "TachibanaEventWs",
    "TickerEventWsHub",
    "is_market_open",
]
