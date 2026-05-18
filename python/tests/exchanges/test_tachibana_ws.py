"""Tests for tachibana_ws (Phase 8 §3.2 A3.1: is_market_open + FdFrameProcessor)."""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal

import pytest

from engine.exchanges.tachibana_ws import FdFrameProcessor, is_market_open

JST = timezone(timedelta(hours=9))


# ---------------------------------------------------------------------------
# is_market_open
# ---------------------------------------------------------------------------


def test_is_market_open_morning_session():
    # 10:00 JST is inside 前場 (09:00–11:30).
    assert is_market_open(datetime(2026, 5, 18, 10, 0, tzinfo=JST)) is True


def test_is_market_open_lunch_break_closed():
    # 12:00 JST is inside 昼休 (11:30–12:30).
    assert is_market_open(datetime(2026, 5, 18, 12, 0, tzinfo=JST)) is False


def test_is_market_open_after_close():
    # 15:35 JST is past クロージング (15:30 end).
    assert is_market_open(datetime(2026, 5, 18, 15, 35, tzinfo=JST)) is False


def test_is_market_open_naive_datetime_treated_as_utc():
    # 00:30 UTC == 09:30 JST → 前場内.
    naive = datetime(2026, 5, 18, 0, 30)
    assert is_market_open(naive) is True


# ---------------------------------------------------------------------------
# FdFrameProcessor — first frame initializes, no trade emitted
# ---------------------------------------------------------------------------


def test_first_frame_initializes_state_returns_no_trade():
    p = FdFrameProcessor(row="1")
    trade, _depth = p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_000_000,
    )
    assert trade is None


# ---------------------------------------------------------------------------
# FdFrameProcessor — DV increase emits a trade with qty = delta
# ---------------------------------------------------------------------------


def test_dv_increase_emits_trade_with_delta_qty():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_000_000,
    )
    trade, _depth = p.process(
        {"p_1_DPP": "3001", "p_1_DV": "1500",
         "p_1_GBP1": "3000", "p_1_GAP1": "3002"},
        recv_ts_ms=1_700_000_001_000,
    )
    assert trade is not None
    assert trade["price"] == "3001"
    assert trade["qty"] == "500"
    # price 3001 >= prev_ask 3001 → buy.
    assert trade["side"] == "buy"
    assert trade["is_liquidation"] is False


# ---------------------------------------------------------------------------
# FdFrameProcessor — DV reset (session rollover) reinitializes without trade
# ---------------------------------------------------------------------------


def test_dv_reset_reinitializes_without_trade():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "5000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_000_000,
    )
    trade, _depth = p.process(
        {"p_1_DPP": "3000", "p_1_DV": "100",  # DV decreased → reset
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_700_000_001_000,
    )
    assert trade is None


# ---------------------------------------------------------------------------
# FdFrameProcessor — depth extraction (bid/ask ladders)
# ---------------------------------------------------------------------------


def test_depth_extracts_bid_ask_ladders():
    p = FdFrameProcessor(row="1")
    fields = {
        "p_1_DPP": "3000", "p_1_DV": "1000",
        "p_1_GBP1": "2999", "p_1_GBV1": "100",
        "p_1_GBP2": "2998", "p_1_GBV2": "200",
        "p_1_GAP1": "3001", "p_1_GAV1": "150",
        "p_1_GAP2": "3002", "p_1_GAV2": "250",
    }
    _trade, depth = p.process(fields, recv_ts_ms=1_700_000_000_000)
    assert depth is not None
    assert depth["bids"] == [
        {"price": "2999", "qty": "100"},
        {"price": "2998", "qty": "200"},
    ]
    assert depth["asks"] == [
        {"price": "3001", "qty": "150"},
        {"price": "3002", "qty": "250"},
    ]
    assert depth["sequence_id"] == 1
    assert depth["recv_ts_ms"] == 1_700_000_000_000


# ---------------------------------------------------------------------------
# FdFrameProcessor — side rules (quote rule + tick rule)
# ---------------------------------------------------------------------------


def test_side_at_or_above_ask_is_buy():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_000,
    )
    trade, _ = p.process(
        {"p_1_DPP": "3001", "p_1_DV": "1100",
         "p_1_GBP1": "3000", "p_1_GAP1": "3002"},
        recv_ts_ms=2_000,
    )
    assert trade is not None and trade["side"] == "buy"


def test_side_at_or_below_bid_is_sell():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_000,
    )
    trade, _ = p.process(
        {"p_1_DPP": "2999", "p_1_DV": "1100",
         "p_1_GBP1": "2998", "p_1_GAP1": "3000"},
        recv_ts_ms=2_000,
    )
    assert trade is not None and trade["side"] == "sell"


# ---------------------------------------------------------------------------
# FdFrameProcessor — reset() clears state so the next frame is treated as first
# ---------------------------------------------------------------------------


def test_reset_clears_prev_state():
    p = FdFrameProcessor(row="1")
    p.process(
        {"p_1_DPP": "3000", "p_1_DV": "1000",
         "p_1_GBP1": "2999", "p_1_GAP1": "3001"},
        recv_ts_ms=1_000,
    )
    p.reset()
    trade, _ = p.process(
        {"p_1_DPP": "3001", "p_1_DV": "9999",
         "p_1_GBP1": "3000", "p_1_GAP1": "3002"},
        recv_ts_ms=2_000,
    )
    # After reset, prev_dv is None again → first frame, no trade despite DV jump.
    assert trade is None


# ---------------------------------------------------------------------------
# TachibanaEventWs — async WS connection manager (Phase 8 §3.2 A3.2a)
# ---------------------------------------------------------------------------

import asyncio
from typing import Any

from engine.exchanges import tachibana_ws as tws_mod
from engine.exchanges.tachibana_ws import TachibanaEventWs


class _FakeWs:
    """Minimal async-iterable fake for websockets.connect() return value.

    Yields the pre-seeded `frames` (bytes or str). After the list is exhausted,
    waits on `idle_event` (or until `auto_close=True`, stops iteration) so the
    test can drive watchdog/stop scenarios without races.
    """

    def __init__(self, frames: list[Any], *, auto_close: bool = True,
                 idle_event: asyncio.Event | None = None) -> None:
        self._frames = list(frames)
        self._auto_close = auto_close
        self._idle = idle_event

    async def __aenter__(self) -> "_FakeWs":
        return self

    async def __aexit__(self, *exc: Any) -> None:
        return None

    def __aiter__(self) -> "_FakeWs":
        return self

    async def __anext__(self) -> Any:
        if self._frames:
            return self._frames.pop(0)
        if self._auto_close:
            raise StopAsyncIteration
        # Park forever (until cancelled) so watchdog can fire.
        if self._idle is not None:
            await self._idle.wait()
        else:
            await asyncio.sleep(3600)
        raise StopAsyncIteration


def _install_fake_ws(monkeypatch, fake: _FakeWs) -> dict[str, Any]:
    """Patch websockets.connect to return `fake`. Returns call-record dict."""
    record: dict[str, Any] = {"calls": []}

    def _fake_connect(url: str, **kwargs: Any) -> _FakeWs:
        record["calls"].append({"url": url, "kwargs": kwargs})
        return fake

    monkeypatch.setattr(tws_mod, "websockets", type("_M", (), {"connect": staticmethod(_fake_connect)}))
    monkeypatch.setattr(tws_mod, "_HAS_WEBSOCKETS", True)
    return record


def _encode_frame_sjis(pairs: list[tuple[str, str]]) -> bytes:
    """Build a raw Shift-JIS event frame matching parse_event_frame's grammar.

    Pair separator = ^A (0x01), key/value separator = ^B (0x02).
    ^C (0x03) は値間 separator (複数値時のみ) であり frame terminator ではない。
    実 server のメッセージ終端は ^A または LF (event_protocol.md L11-12)。
    """
    body = "\x01".join(f"{k}\x02{v}" for k, v in pairs)
    return body.encode("cp932")


async def test_event_ws_dispatches_kp_and_fd_frames(monkeypatch):
    stop = asyncio.Event()
    received: list[tuple[str, dict[str, str]]] = []

    async def cb(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received.append((frame_type, fields))
        if len(received) >= 2:
            stop.set()

    frames = [
        _encode_frame_sjis([("p_cmd", "KP")]),
        _encode_frame_sjis([("p_cmd", "FD"), ("p_1_DPP", "3000"), ("p_1_DV", "1000")]),
    ]
    fake = _FakeWs(frames, auto_close=False, idle_event=stop)
    rec = _install_fake_ws(monkeypatch, fake)

    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")
    await asyncio.wait_for(ws.run(cb), timeout=2.0)

    assert [t for t, _ in received] == ["KP", "FD"]
    assert received[1][1]["p_1_DPP"] == "3000"
    # ping_interval=None must always be passed (立花 manual ping/pong).
    assert rec["calls"][0]["kwargs"].get("ping_interval") is None


async def test_event_ws_ping_interval_none_passed(monkeypatch):
    stop = asyncio.Event()

    async def cb(*_a, **_k) -> None:
        stop.set()

    fake = _FakeWs([_encode_frame_sjis([("p_cmd", "KP")])], auto_close=False, idle_event=stop)
    rec = _install_fake_ws(monkeypatch, fake)
    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")
    await asyncio.wait_for(ws.run(cb), timeout=2.0)
    assert rec["calls"][0]["kwargs"]["ping_interval"] is None


async def test_event_ws_proxy_kwarg_forwarded(monkeypatch):
    stop = asyncio.Event()

    async def cb(*_a, **_k) -> None:
        stop.set()

    fake = _FakeWs([_encode_frame_sjis([("p_cmd", "KP")])], auto_close=False, idle_event=stop)
    rec = _install_fake_ws(monkeypatch, fake)
    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203",
                          proxy="http://127.0.0.1:8080")
    await asyncio.wait_for(ws.run(cb), timeout=2.0)
    assert rec["calls"][0]["kwargs"].get("proxy") == "http://127.0.0.1:8080"


async def test_event_ws_stop_event_terminates_run(monkeypatch):
    stop = asyncio.Event()
    seen: list[str] = []

    async def cb(frame_type: str, *_a) -> None:
        seen.append(frame_type)
        stop.set()

    fake = _FakeWs([_encode_frame_sjis([("p_cmd", "KP")])], auto_close=False, idle_event=stop)
    _install_fake_ws(monkeypatch, fake)
    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")
    await asyncio.wait_for(ws.run(cb), timeout=1.5)
    assert seen == ["KP"]


async def test_event_ws_dead_frame_timeout_triggers_reconnect(monkeypatch):
    monkeypatch.setattr(tws_mod, "_DEAD_FRAME_TIMEOUT_S", 0.2)
    monkeypatch.setattr(tws_mod, "_BACKOFF_CAPS", (0.01,))

    stop = asyncio.Event()
    connect_counter = {"n": 0}

    # Each connect returns a fresh fake that yields no frames and parks → watchdog fires.
    def _make_fake() -> _FakeWs:
        connect_counter["n"] += 1
        return _FakeWs([], auto_close=False, idle_event=asyncio.Event())

    def _fake_connect(url: str, **kwargs: Any) -> _FakeWs:
        return _make_fake()

    monkeypatch.setattr(tws_mod, "websockets",
                        type("_M", (), {"connect": staticmethod(_fake_connect)}))
    monkeypatch.setattr(tws_mod, "_HAS_WEBSOCKETS", True)

    async def cb(*_a, **_k) -> None: ...

    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")

    async def _stopper() -> None:
        await asyncio.sleep(0.6)  # allow at least 2 reconnects
        stop.set()

    await asyncio.gather(ws.run(cb), _stopper())
    # At least one reconnect happened (initial + 1+ after timeout).
    assert connect_counter["n"] >= 2


async def test_event_ws_on_connect_callback_invoked_each_connection(monkeypatch):
    monkeypatch.setattr(tws_mod, "_DEAD_FRAME_TIMEOUT_S", 0.2)
    monkeypatch.setattr(tws_mod, "_BACKOFF_CAPS", (0.01,))

    stop = asyncio.Event()
    on_connect_calls = {"n": 0}

    def _on_connect() -> None:
        on_connect_calls["n"] += 1

    def _fake_connect(url: str, **kwargs: Any) -> _FakeWs:
        return _FakeWs([], auto_close=False, idle_event=asyncio.Event())

    monkeypatch.setattr(tws_mod, "websockets",
                        type("_M", (), {"connect": staticmethod(_fake_connect)}))
    monkeypatch.setattr(tws_mod, "_HAS_WEBSOCKETS", True)

    async def cb(*_a, **_k) -> None: ...

    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")

    async def _stopper() -> None:
        await asyncio.sleep(0.6)
        stop.set()

    await asyncio.gather(ws.run(cb, on_connect=_on_connect), _stopper())
    assert on_connect_calls["n"] >= 2


async def test_event_ws_raises_without_websockets_package(monkeypatch):
    monkeypatch.setattr(tws_mod, "_HAS_WEBSOCKETS", False)
    stop = asyncio.Event()
    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")

    async def cb(*_a, **_k) -> None: ...

    with pytest.raises(RuntimeError, match="websockets"):
        await ws.run(cb)


# ---------------------------------------------------------------------------
# TickerEventWsHub — multi-subscriber multiplexer (Phase 8 §3.2 A3.2b)
# ---------------------------------------------------------------------------

from engine.exchanges.tachibana_ws import TickerEventWsHub


async def test_hub_subscribe_starts_ws_and_unsubscribe_stops(monkeypatch):
    """最初の subscribe で WS runner task が起動し、最後の unsubscribe で停止する。"""
    stop_hint = asyncio.Event()
    fake = _FakeWs(
        [_encode_frame_sjis([("p_cmd", "KP")])],
        auto_close=False,
        idle_event=stop_hint,
    )
    _install_fake_ws(monkeypatch, fake)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")
    assert hub.subscriber_count == 0

    received: list[str] = []

    async def cb(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received.append(frame_type)

    await hub.subscribe("sub-a", cb)
    assert hub.subscriber_count == 1
    # WS タスクが起動したことを KP frame が cb に届くことで観測。
    await asyncio.wait_for(
        _wait_until(lambda: received == ["KP"]), timeout=2.0
    )

    await hub.unsubscribe("sub-a")
    assert hub.subscriber_count == 0
    # _FakeWs を park させていた idle_event を解放して runner を畳ませる。
    stop_hint.set()
    # unsubscribe が最後の subscriber を外したら runner task は短時間で終わる。
    await asyncio.wait_for(_wait_runner_done(hub), timeout=2.0)


async def _wait_until(pred, *, interval: float = 0.01) -> None:
    while not pred():
        await asyncio.sleep(interval)


async def _wait_runner_done(hub) -> None:
    # private 属性に触るのは hub の lifecycle 観測 (test scope) のためのみ。
    task = hub._runner_task
    if task is None:
        return
    while not task.done():
        await asyncio.sleep(0.01)
