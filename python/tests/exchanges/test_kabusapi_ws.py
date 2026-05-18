"""Tests for kabusapi_ws.connect() (Phase 8 §3.2 B4-2).

websockets.connect を _FakeWs で差し替え、ws.recv() を直接呼ぶ
（e-station 写経元と同じく、__aiter__ ではなく ws.recv() 直接呼びループ）。

注:
- asyncio_mode="auto" 設定済なので async def test_ にマーク不要。
- SUT (engine.exchanges.kabusapi_ws) は B4-2 RED 時点で未実装のため、
  各 test 内で deferred import する（top-level import すると collection error）。
"""
from __future__ import annotations

import asyncio
import json
from typing import Any

import pytest
import websockets.exceptions

from engine.exchanges.kabusapi_auth import KabuConnectionError
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

    assert record["n"] == 5, "OSError は 5 回で打ち切るはず"


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

    # 1..5 回目は通過、6 回目で raise → 計 6 connect
    assert record["n"] == 6


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
