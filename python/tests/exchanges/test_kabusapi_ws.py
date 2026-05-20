"""Tests for kabusapi_ws.connect().

websockets.connect を _FakeWs で差し替え、ws.recv() を直接呼ぶ
（__aiter__ ではなく ws.recv() 直接呼びループ）。

asyncio_mode="auto" 設定済なので async def test_ にマーク不要。
"""
from __future__ import annotations

import asyncio
import json
from typing import Any

import pytest
import websockets.exceptions

from engine.exchanges.kabusapi_auth import (
    KabuConnectionError,
    KabuRateLimitError,
    KabuTokenExpiredError,
)
from engine.exchanges.kabusapi_register import RegisterSet


class _FakeWs:
    """Minimal async-context-manager fake for websockets.connect().

    Yields the pre-seeded `frames` (bytes or str) on successive ws.recv() calls.
    After frames are exhausted:
      - if `close_exc` is set, raise it (simulates ConnectionClosed*).
      - else park forever (until cancelled) so the test can drive timeout/stop.
    """

    def __init__(
        self,
        frames: list[Any],
        *,
        close_exc: BaseException | None = None,
    ) -> None:
        self._frames = list(frames)
        self._close_exc = close_exc

    async def __aenter__(self) -> "_FakeWs":
        return self

    async def __aexit__(self, *exc: Any) -> None:
        return None

    async def recv(self) -> Any:
        if self._frames:
            return self._frames.pop(0)
        if self._close_exc is not None:
            raise self._close_exc
        # Park forever — caller is expected to wrap in asyncio.wait_for / timeout.
        await asyncio.sleep(3600)
        raise RuntimeError("unreachable")


def _install_fake_ws(monkeypatch, kws_mod, factory) -> dict[str, Any]:
    """Patch kws_mod.websockets.connect to call `factory(url, **kwargs)`.

    Returns a record dict with `calls` (list of {url, kwargs}).
    """
    record: dict[str, Any] = {"calls": []}

    def _fake_connect(url: str, **kwargs: Any):
        record["calls"].append({"url": url, "kwargs": kwargs})
        return factory(url, **kwargs)

    # websockets.exceptions も保持しておく（SUT が websockets.exceptions.X を参照するため）。
    fake_mod = type(
        "_M",
        (),
        {
            "connect": staticmethod(_fake_connect),
            "exceptions": websockets.exceptions,
        },
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)
    return record


# ---------------------------------------------------------------------------
# 1. ping_interval=None and compression=None must be passed to websockets.connect
# ---------------------------------------------------------------------------


async def test_connect_passes_ping_interval_none_and_compression_none(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    received: list[dict] = []

    async def on_message(msg: dict) -> None:
        received.append(msg)
        raise asyncio.CancelledError  # 1 frame で抜ける

    # 1 frame だけ配って、その後 on_message が CancelledError で connect() を抜ける。
    frame = json.dumps({"hello": "world"})
    fake = _FakeWs([frame])
    rec = _install_fake_ws(monkeypatch, kws_mod, lambda *_a, **_k: fake)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    assert len(rec["calls"]) >= 1
    kwargs = rec["calls"][0]["kwargs"]
    assert kwargs.get("ping_interval") is None, (
        "kabuStation は RFC6455 非準拠 PONG なので keepalive 無効化必須"
    )
    assert kwargs.get("compression") is None, (
        "kabuStation の permessage-deflate RSV1 バグ回避のため compression=None 必須"
    )


# ---------------------------------------------------------------------------
# 2. Empty RegisterSet → put_register must NOT be called
# ---------------------------------------------------------------------------


async def test_connect_with_empty_register_set_does_not_call_put_register(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    rs = RegisterSet()  # empty
    put_register_called = {"n": 0}

    async def put_register(symbols):
        put_register_called["n"] += 1
        return True

    async def on_message(msg: dict) -> None:
        raise asyncio.CancelledError

    fake = _FakeWs([json.dumps({"x": 1})])
    _install_fake_ws(monkeypatch, kws_mod, lambda *_a, **_k: fake)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    assert put_register_called["n"] == 0, "空 RegisterSet で put_register を呼んではいけない"


# ---------------------------------------------------------------------------
# 3. Non-empty RegisterSet → put_register(symbols) is called on connect
# ---------------------------------------------------------------------------


async def test_connect_with_symbols_calls_put_register_with_all_symbols(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    rs = RegisterSet()
    rs.register("7203", 1)
    rs.register("9984", 1)

    put_calls: list[list[tuple[str, int]]] = []

    async def put_register(symbols):
        put_calls.append(list(symbols))
        return True

    async def on_message(msg: dict) -> None:
        raise asyncio.CancelledError

    fake = _FakeWs([json.dumps({"x": 1})])
    _install_fake_ws(monkeypatch, kws_mod, lambda *_a, **_k: fake)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    assert len(put_calls) == 1
    assert put_calls[0] == [("7203", 1), ("9984", 1)]


# ---------------------------------------------------------------------------
# 4. Frame is JSON-parsed and on_message receives a dict
# ---------------------------------------------------------------------------


async def test_connect_parses_json_frame_and_dispatches_dict_to_on_message(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    received: list[dict] = []

    async def on_message(msg: dict) -> None:
        received.append(msg)
        raise asyncio.CancelledError

    payload = {"Symbol": "7203", "CurrentPrice": 3000.5}
    fake = _FakeWs([json.dumps(payload)])
    _install_fake_ws(monkeypatch, kws_mod, lambda *_a, **_k: fake)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    assert received == [payload]
    assert isinstance(received[0], dict)


# ---------------------------------------------------------------------------
# 5. UTF-8 bytes frame is decoded (SJIS-reject の裏返し: utf-8 で decode する)
# ---------------------------------------------------------------------------


async def test_connect_decodes_utf8_bytes_frame(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    received: list[dict] = []

    async def on_message(msg: dict) -> None:
        received.append(msg)
        raise asyncio.CancelledError

    payload = {"k": "v"}
    raw = json.dumps(payload).encode("utf-8")
    fake = _FakeWs([raw])
    _install_fake_ws(monkeypatch, kws_mod, lambda *_a, **_k: fake)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    assert received == [payload]


# ---------------------------------------------------------------------------
# 6. OSError 5 連続 → KabuConnectionError raise
# ---------------------------------------------------------------------------


async def test_connect_raises_kabu_connection_error_after_5_consecutive_oserrors(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    # sleep を 0 にして高速化
    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    async def on_message(msg: dict) -> None:
        pass

    def _factory(*_a, **_k):
        raise OSError("connection refused")

    # _install_fake_ws を使うと factory が __aenter__ で例外を出せないため、
    # connect() 自体が raise する形にする。
    record = {"n": 0}

    def _fake_connect(url: str, **kwargs: Any):
        record["n"] += 1
        raise OSError("connection refused")

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    with pytest.raises(KabuConnectionError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=5.0,
        )

    assert record["n"] == kws_mod._MAX_RECONNECT_ATTEMPTS, (
        "OSError は _MAX_RECONNECT_ATTEMPTS 回で打ち切るはず"
    )


# ---------------------------------------------------------------------------
# 7. ConnectionClosedOK > 5 回 (= 6 回目) で KabuConnectionError raise
# ---------------------------------------------------------------------------


async def test_connect_raises_after_six_consecutive_connection_closed_ok(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    async def on_message(msg: dict) -> None:
        pass

    record = {"n": 0}

    def _make_ws() -> _FakeWs:
        record["n"] += 1
        # frames 無し → recv で ConnectionClosedOK
        return _FakeWs(
            [],
            close_exc=websockets.exceptions.ConnectionClosedOK(None, None),
        )

    def _fake_connect(url: str, **kwargs: Any):
        return _make_ws()

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    with pytest.raises(KabuConnectionError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=5.0,
        )

    # > _MAX_RECONNECT_ATTEMPTS で raise → 計 _MAX + 1 connect
    assert record["n"] == kws_mod._MAX_RECONNECT_ATTEMPTS + 1


# ---------------------------------------------------------------------------
# 8. URL に "websocket" が含まれる (verify env)
# ---------------------------------------------------------------------------


async def test_connect_uses_ws_url_for_verify_env(monkeypatch):
    from engine.exchanges import kabusapi_ws as kws_mod

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    async def on_message(msg: dict) -> None:
        raise asyncio.CancelledError

    fake = _FakeWs([json.dumps({"x": 1})])
    rec = _install_fake_ws(monkeypatch, kws_mod, lambda *_a, **_k: fake)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    url = rec["calls"][0]["url"]
    assert url.startswith("ws://"), f"ws:// scheme expected, got {url}"
    assert "18081" in url, "verify env は 18081 ポート"
    assert url.endswith("/websocket"), f"/websocket 終端であるべき: {url}"


# ---------------------------------------------------------------------------
# 9. handshake 成功 → recv 即 ConnectionClosedError を繰り返すケースで
#    consecutive_failures が毎回リセットされず、6 回目で打ち切る
# ---------------------------------------------------------------------------


async def test_connect_resets_failures_only_after_first_successful_frame(monkeypatch):
    """High バグ回帰防止:

    handshake (`async with websockets.connect(...)`) は成功するが、
    最初の ws.recv() で ConnectionClosedError が即上がるシナリオ。
    現行実装は `async with` 直後に `consecutive_failures = 0` を実行するため、
    毎接続でカウンタがゼロに戻り、_MAX_RECONNECT_ATTEMPTS (5) に到達せず
    無限ループする。fix では「最初の frame を on_message に dispatch した時点」
    まで reset を遅延し、本テストは 6 回目の試行で KabuConnectionError を観測する。
    """
    from engine.exchanges import kabusapi_ws as kws_mod

    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    async def on_message(msg: dict) -> None:
        # frame は届かないので呼ばれない
        pass

    record = {"n": 0}

    def _make_ws() -> _FakeWs:
        record["n"] += 1
        # frames=[] + close_exc=ConnectionClosedError →
        # __aenter__ は成功するが recv 即 ConnectionClosedError
        return _FakeWs(
            [],
            close_exc=websockets.exceptions.ConnectionClosedError(None, None),
        )

    def _fake_connect(url: str, **kwargs: Any):
        return _make_ws()

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    with pytest.raises(KabuConnectionError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=5.0,
        )

    # >= _MAX_RECONNECT_ATTEMPTS で raise → 計 _MAX connect
    assert record["n"] == kws_mod._MAX_RECONNECT_ATTEMPTS


# ===========================================================================
# Post-merge review fixes (2026-05-20)
# ===========================================================================


async def test_connect_invokes_on_reconnect_before_replaying_register(monkeypatch):
    """HIGH-3: on the *2nd* connect (i.e. reconnect), the on_reconnect hook
    must be invoked BEFORE put_register replays symbols, so processors are
    reset before any new frame is dispatched."""
    from engine.exchanges import kabusapi_ws as kws_mod

    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()
    rs.register("7203", 1)

    events_log: list[str] = []

    async def put_register(symbols):
        events_log.append("put_register")
        return True

    async def on_reconnect():
        events_log.append("on_reconnect")

    async def on_message(msg: dict) -> None:
        raise asyncio.CancelledError

    # First _FakeWs raises ConnectionClosedOK immediately to force reconnect;
    # second yields one frame and on_message cancels.
    state = {"call": 0}

    def _make_ws():
        state["call"] += 1
        if state["call"] == 1:
            return _FakeWs(
                [],
                close_exc=websockets.exceptions.ConnectionClosedOK(None, None),
            )
        return _FakeWs([json.dumps({"x": 1})])

    def _fake_connect(url: str, **kwargs: Any):
        return _make_ws()

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
                on_reconnect=on_reconnect,
            ),
            timeout=2.0,
        )

    # on_reconnect must precede the 2nd put_register
    assert "on_reconnect" in events_log
    or_idx = events_log.index("on_reconnect")
    # put_register on the *reconnect* attempt comes after on_reconnect
    later_put = [
        i for i, e in enumerate(events_log) if e == "put_register" and i > or_idx
    ]
    assert later_put, f"put_register should follow on_reconnect; got {events_log}"


async def test_connect_does_not_increment_failures_on_json_decode_error(monkeypatch):
    """MEDIUM-4: a JSONDecodeError from a malformed frame must be logged at
    warning level and NOT incremented into the reconnect failure counter."""
    from engine.exchanges import kabusapi_ws as kws_mod

    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()

    async def put_register(symbols):
        return True

    call_count = {"n": 0}

    async def on_message(msg: dict) -> None:
        call_count["n"] += 1
        raise asyncio.CancelledError

    # First frame is malformed JSON, second is valid → on_message cancels.
    fake = _FakeWs(["not-a-json{", json.dumps({"x": 1})])
    record = {"n": 0}

    def _fake_connect(url: str, **kwargs: Any):
        record["n"] += 1
        return fake

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    # If JSON decode error had incremented failures, the loop would have
    # disconnected/reconnected. Verify single connect call & on_message
    # eventually got the valid frame.
    assert record["n"] == 1, "JSONDecodeError must not trigger reconnect"
    assert call_count["n"] == 1


async def test_connect_propagates_fatal_put_register_error_on_reconnect(monkeypatch):
    """Round1 HIGH: a non-transient put_register failure during reconnect
    replay (e.g. KabuTokenExpiredError) must NOT be swallowed. It propagates
    out of connect() so _run_ws can capture it into adapter.last_error and the
    dead session becomes observable instead of "connected but silent"."""
    from engine.exchanges import kabusapi_ws as kws_mod

    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()
    rs.register("7203", 1)

    put_calls = {"n": 0}

    async def put_register(symbols):
        put_calls["n"] += 1
        # First connect: succeed. Reconnect replay: token expired.
        if put_calls["n"] >= 2:
            raise KabuTokenExpiredError(4001005, "token expired")
        return True

    async def on_message(msg: dict) -> None:
        raise asyncio.CancelledError

    # First ws: closes OK immediately to force a reconnect. Second ws would
    # yield a frame, but the replay raises before we ever recv().
    state = {"call": 0}

    def _make_ws():
        state["call"] += 1
        if state["call"] == 1:
            return _FakeWs(
                [],
                close_exc=websockets.exceptions.ConnectionClosedOK(None, None),
            )
        return _FakeWs([json.dumps({"x": 1})])

    def _fake_connect(url: str, **kwargs: Any):
        return _make_ws()

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    with pytest.raises(KabuTokenExpiredError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )


async def test_connect_swallows_rate_limit_put_register_error_on_reconnect(monkeypatch):
    """Round1 HIGH (transient branch): a KabuRateLimitError during reconnect
    replay is transient — warn and keep the reconnect loop alive rather than
    tearing the session down. The session proceeds to recv() the next frame."""
    from engine.exchanges import kabusapi_ws as kws_mod

    monkeypatch.setattr(kws_mod, "_RECONNECT_DELAY_S", 0.0)

    rs = RegisterSet()
    rs.register("7203", 1)

    put_calls = {"n": 0}

    async def put_register(symbols):
        put_calls["n"] += 1
        if put_calls["n"] >= 2:
            raise KabuRateLimitError(4002006, "rate limited")
        return True

    received: list[dict] = []

    async def on_message(msg: dict) -> None:
        received.append(msg)
        raise asyncio.CancelledError

    state = {"call": 0}

    def _make_ws():
        state["call"] += 1
        if state["call"] == 1:
            return _FakeWs(
                [],
                close_exc=websockets.exceptions.ConnectionClosedOK(None, None),
            )
        return _FakeWs([json.dumps({"x": 1})])

    def _fake_connect(url: str, **kwargs: Any):
        return _make_ws()

    fake_mod = type(
        "_M",
        (),
        {"connect": staticmethod(_fake_connect), "exceptions": websockets.exceptions},
    )
    monkeypatch.setattr(kws_mod, "websockets", fake_mod)

    # Rate limit is swallowed → recv() proceeds → on_message cancels.
    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(
            kws_mod.connect(
                env="verify",
                on_message=on_message,
                register_set=rs,
                put_register=put_register,
            ),
            timeout=2.0,
        )

    assert received == [{"x": 1}], "rate-limit replay must not abort the session"
