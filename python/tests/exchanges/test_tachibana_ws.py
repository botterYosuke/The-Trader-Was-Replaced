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


async def test_event_ws_normal_close_triggers_backoff_not_storm(monkeypatch):
    """Bug fix: 正常 close (StopAsyncIteration) でも backoff に乗せ、
    reconnect storm を防ぐ。0.2s 中 reconnect 回数が爆発しないこと。"""
    monkeypatch.setattr(tws_mod, "_BACKOFF_CAPS", (0.05,))

    stop = asyncio.Event()
    connect_counter = {"n": 0}

    # auto_close=True で frame 配り終わったら即 StopAsyncIteration → 正常 close.
    def _fake_connect(url: str, **kwargs: Any) -> _FakeWs:
        connect_counter["n"] += 1
        return _FakeWs([], auto_close=True)

    monkeypatch.setattr(tws_mod, "websockets",
                        type("_M", (), {"connect": staticmethod(_fake_connect)}))
    monkeypatch.setattr(tws_mod, "_HAS_WEBSOCKETS", True)

    async def cb(*_a, **_k) -> None: ...

    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")

    async def _stopper() -> None:
        await asyncio.sleep(0.2)
        stop.set()

    await asyncio.gather(ws.run(cb), _stopper())
    # backoff=0.05s なので 0.2s 中 reconnect は最大 ~5 回程度。
    # 現状 (バグ) は backoff なしで 1000+ 回連結する。10 回を上限とする。
    assert connect_counter["n"] <= 10, (
        f"reconnect storm detected: {connect_counter['n']} connects in 0.2s"
    )


async def test_event_ws_proxy_none_passed_explicitly(monkeypatch):
    """Bug fix: proxy 未指定でも connect_kwargs に proxy=None を明示渡し、
    websockets 16.0 の env-proxy 自動採用を抑止する。"""
    stop = asyncio.Event()

    async def cb(*_a, **_k) -> None:
        stop.set()

    fake = _FakeWs([_encode_frame_sjis([("p_cmd", "KP")])],
                   auto_close=False, idle_event=stop)
    rec = _install_fake_ws(monkeypatch, fake)
    ws = TachibanaEventWs("wss://example/ws", stop, ticker="7203")
    await asyncio.wait_for(ws.run(cb), timeout=2.0)
    # proxy kwarg は呼び出しに存在し、かつ値が None であること。
    kwargs = rec["calls"][0]["kwargs"]
    assert "proxy" in kwargs, "proxy kwarg must be passed explicitly"
    assert kwargs["proxy"] is None


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


async def test_hub_duplicate_subscribe_is_noop(monkeypatch):
    """同じ key の 2 回目 subscribe は警告ログだけで no-op、count は増えない。"""
    stop_hint = asyncio.Event()
    fake = _FakeWs(
        [_encode_frame_sjis([("p_cmd", "KP")])],
        auto_close=False,
        idle_event=stop_hint,
    )
    _install_fake_ws(monkeypatch, fake)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")

    received: list[str] = []

    async def cb_first(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received.append(f"first:{frame_type}")

    async def cb_second(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received.append(f"second:{frame_type}")

    await hub.subscribe("dup-key", cb_first)
    await hub.subscribe("dup-key", cb_second)  # 同 key → no-op
    assert hub.subscriber_count == 1

    # frame を流すと cb_first だけ発火 (cb_second は登録されていない)。
    await asyncio.wait_for(
        _wait_until(lambda: received == ["first:KP"]), timeout=2.0
    )

    await hub.aclose()
    stop_hint.set()
    await asyncio.wait_for(_wait_runner_done(hub), timeout=2.0)


async def test_hub_fanout_delivers_frame_to_all_subscribers_in_order(monkeypatch):
    """2 subscriber に同じ frame が登録順で配られる。"""
    stop_hint = asyncio.Event()
    fake = _FakeWs(
        [_encode_frame_sjis([("p_cmd", "KP")])],
        auto_close=False,
        idle_event=stop_hint,
    )
    _install_fake_ws(monkeypatch, fake)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")
    order: list[str] = []

    async def cb_a(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        order.append(f"a:{frame_type}")

    async def cb_b(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        order.append(f"b:{frame_type}")

    await hub.subscribe("sub-a", cb_a)
    await hub.subscribe("sub-b", cb_b)
    assert hub.subscriber_count == 2

    await asyncio.wait_for(
        _wait_until(lambda: order == ["a:KP", "b:KP"]), timeout=2.0
    )

    await hub.aclose()
    stop_hint.set()
    await asyncio.wait_for(_wait_runner_done(hub), timeout=2.0)


async def test_hub_aclose_fires_on_close_for_all_subscribers_and_clears(monkeypatch):
    """aclose() で全 subscriber の on_close が発火し、subscribers クリア + runner 終了。"""
    stop_hint = asyncio.Event()
    fake = _FakeWs(
        [_encode_frame_sjis([("p_cmd", "KP")])],
        auto_close=False,
        idle_event=stop_hint,
    )
    _install_fake_ws(monkeypatch, fake)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")
    closed: list[str] = []

    async def cb(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        pass

    def on_close_a() -> None:
        closed.append("a")

    def on_close_b() -> None:
        closed.append("b")

    await hub.subscribe("sub-a", cb, on_close=on_close_a)
    await hub.subscribe("sub-b", cb, on_close=on_close_b)
    assert hub.subscriber_count == 2

    # runner が走り出すのを 1 frame の到達で観測してから aclose する。
    received_marker = asyncio.Event()

    async def cb_marker(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received_marker.set()

    await hub.subscribe("marker", cb_marker)
    await asyncio.wait_for(received_marker.wait(), timeout=2.0)

    stop_hint.set()  # _FakeWs を park から解放しておく
    await hub.aclose()

    assert sorted(closed) == ["a", "b"]
    assert hub.subscriber_count == 0
    assert hub._runner_task is None or hub._runner_task.done()


async def test_hub_on_connect_invoked_on_ws_connection(monkeypatch):
    """subscribe 時に渡した on_connect が WS 接続時 (_on_ws_connect 経由) に呼ばれる。"""
    stop_hint = asyncio.Event()
    fake = _FakeWs(
        [_encode_frame_sjis([("p_cmd", "KP")])],
        auto_close=False,
        idle_event=stop_hint,
    )
    _install_fake_ws(monkeypatch, fake)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")
    connect_calls: list[str] = []

    async def cb(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        pass

    def on_connect_a() -> None:
        connect_calls.append("a")

    await hub.subscribe("sub-a", cb, on_connect=on_connect_a)

    # WS が接続して KP が cb に届くまで待てば on_connect も呼ばれているはず。
    await asyncio.wait_for(
        _wait_until(lambda: connect_calls == ["a"]), timeout=2.0
    )

    await hub.aclose()
    stop_hint.set()
    await asyncio.wait_for(_wait_runner_done(hub), timeout=2.0)


async def test_hub_dispatch_exception_does_not_propagate_to_other_subscribers(monkeypatch):
    """1 subscriber の cb が raise しても他 subscriber に伝播せず両方呼ばれる。"""
    stop_hint = asyncio.Event()
    fake = _FakeWs(
        [_encode_frame_sjis([("p_cmd", "KP")])],
        auto_close=False,
        idle_event=stop_hint,
    )
    _install_fake_ws(monkeypatch, fake)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")
    received: list[str] = []

    async def cb_bad(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received.append("bad-entered")
        raise RuntimeError("intentional dispatch failure")

    async def cb_good(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received.append("good-received")

    await hub.subscribe("sub-bad", cb_bad)
    await hub.subscribe("sub-good", cb_good)

    # 登録順は bad → good。bad が raise しても good に届くこと。
    await asyncio.wait_for(
        _wait_until(lambda: "good-received" in received), timeout=2.0
    )
    assert "bad-entered" in received

    await hub.aclose()
    stop_hint.set()
    await asyncio.wait_for(_wait_runner_done(hub), timeout=2.0)


async def test_hub_resubscribe_after_unsubscribe_restarts_runner(monkeypatch):
    """Race: unsubscribe で stop_event=set した直後、既存 runner が park から
    抜ける前に subscribe('b') が来ると、現状実装は ``runner_task.done() is False``
    のため restart を skip し、結果として新 subscriber に frame が届かない。

    再現手順:
      1. subscribe('a') で runner 起動 → _FakeWs#1 は frame 配り終えて idle park
      2. unsubscribe('a') → stop_event.set() するが runner_task はまだ park 中で pending
      3. 即 subscribe('b') を呼ぶ (await 1 step のみ)
      4. idle_event を release → 既存 runner は stop_event=True を見て終了
         → bug 時: 新 runner は立たず connect_counter == 1 / received == []
         → fix 後: stop_event.is_set() を検知して新 runner 起動 → connect_counter == 2 / received == ["KP"]
    """
    # 接続毎に新しい _FakeWs を返し、connect 回数を観測する。
    connect_counter = {"n": 0}
    idle_events: list[asyncio.Event] = []
    fakes: list[_FakeWs] = []

    def _fake_connect(url: str, **kwargs: Any) -> _FakeWs:
        connect_counter["n"] += 1
        idle = asyncio.Event()
        idle_events.append(idle)
        fake = _FakeWs(
            [_encode_frame_sjis([("p_cmd", "KP")])],
            auto_close=False,
            idle_event=idle,
        )
        fakes.append(fake)
        return fake

    monkeypatch.setattr(
        tws_mod, "websockets",
        type("_M", (), {"connect": staticmethod(_fake_connect)}),
    )
    monkeypatch.setattr(tws_mod, "_HAS_WEBSOCKETS", True)

    hub = TickerEventWsHub("wss://example/ws", ticker="7203")
    received_a: list[str] = []
    received_b: list[str] = []

    async def cb_a(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received_a.append(frame_type)

    async def cb_b(frame_type: str, fields: dict[str, str], recv_ts_ms: int) -> None:
        received_b.append(frame_type)

    # 1) subscribe('a') → 1 本目接続 + KP 受信 + idle park.
    await hub.subscribe("sub-a", cb_a)
    await asyncio.wait_for(
        _wait_until(lambda: received_a == ["KP"]), timeout=2.0
    )
    assert connect_counter["n"] == 1

    # 2) unsubscribe('a') → stop_event.set される (runner はまだ park 中で pending).
    await hub.unsubscribe("sub-a")
    assert hub._runner_task is not None
    assert not hub._runner_task.done(), (
        "precondition: runner_task should still be pending (parked on idle_event)"
    )
    assert hub._stop_event.is_set(), (
        "precondition: unsubscribe() should have set stop_event"
    )

    # 3) 直後に subscribe('b'). 現状実装は runner_task.done() is False のため
    #    restart を skip する (= bug).
    await hub.subscribe("sub-b", cb_b)

    # 4) 既存 runner を park から解放 → stop_event=True を見て自然終了させる.
    for ev in idle_events:
        ev.set()

    # bug 時: 新 runner 不在のため connect は 1 回のまま, received_b は空.
    # fix 後: 2 本目接続が走り KP が cb_b に届く.
    await asyncio.wait_for(
        _wait_until(lambda: received_b == ["KP"]), timeout=2.0
    )
    assert connect_counter["n"] == 2, (
        f"new runner should reconnect, got connect_counter={connect_counter['n']}"
    )

    await hub.aclose()
