"""kabu STATION PUSH WebSocket connection manager.

Responsibility:
- websockets.connect(url, ping_interval=None, compression=None) で接続
  (kabuStation は RFC6455 非準拠 PONG + permessage-deflate RSV1 バグがあるため
   keepalive と圧縮を無効化する)
- 接続直後に register_set の銘柄を put_register で再送
  - replay が KabuRateLimitError なら warning のみで loop 継続 (transient)
  - それ以外の put_register 失敗 (token 失効 / register full / 5xx / network)
    は再送できないので swallow せず伝播させ、_run_ws → adapter.last_error →
    events() で観測可能にする (Round1 HIGH: "connected but silent" 回避)
- 再接続時 (2 回目以降の connect) に on_reconnect callback を発火し、
  caller (adapter) が processor の state を reset できるようにする (HIGH-3)
- ws.recv() ループで JSON frame を on_message に dispatch
- OSError / ConnectionClosedError は consecutive_failures を増やし、
  >= _MAX_RECONNECT_ATTEMPTS で KabuConnectionError raise
- ConnectionClosedOK は consecutive_ok_close_count を増やし、
  > _MAX_RECONNECT_ATTEMPTS で KabuConnectionError raise
- JSONDecodeError / on_message 例外は warning ログのみで failure を増やさない
  (MEDIUM-4: network 系と codec/user-callback 系を区別する)
- 各 except 後は _RECONNECT_DELAY_S sleep して再接続

asyncio.CancelledError は BaseException なので except Exception で
捕まらずそのまま伝播する (caller の停止経路を阻害しない).
"""
from __future__ import annotations

import asyncio
import json
import logging
from typing import Awaitable, Callable, Optional

import websockets
import websockets.exceptions

from engine.exchanges.kabusapi_auth import KabuConnectionError, KabuRateLimitError
from engine.exchanges.kabusapi_register import RegisterSet
from engine.exchanges.kabusapi_url import KabuEnv, ws_url

logger = logging.getLogger(__name__)

# Tunables — tests monkeypatch these to accelerate.
_RECONNECT_DELAY_S: float = 5.0
_MAX_RECONNECT_ATTEMPTS: int = 5
_RECV_TIMEOUT_S: float = 3600.0


async def connect(
    *,
    env: KabuEnv,
    on_message: Callable[[dict], Awaitable[None] | None],
    register_set: RegisterSet,
    put_register: Callable[[list[tuple[str, int]]], Awaitable[bool]],
    on_reconnect: Optional[Callable[[], Awaitable[None] | None]] = None,
) -> None:
    """Manage a kabu PUSH WebSocket session with auto-reconnect.

    Args:
        on_reconnect: optional callback fired on *re*connect (i.e. the 2nd+
            successful `websockets.connect`). The caller resets per-symbol
            processor state here so the first post-reconnect frame is treated
            as a fresh snapshot (HIGH-3). Not called on the very first connect.

    Raises KabuConnectionError when reconnect attempts exceed
    _MAX_RECONNECT_ATTEMPTS. Returns only via cancellation / caller-raised
    BaseException (e.g. on_message raising CancelledError).
    """
    url = ws_url(env)
    consecutive_failures = 0
    consecutive_ok_close_count = 0
    is_first_connect = True

    while True:
        try:
            async with websockets.connect(
                url,
                ping_interval=None,
                compression=None,
            ) as ws:
                if not is_first_connect and on_reconnect is not None:
                    result = on_reconnect()
                    if asyncio.iscoroutine(result):
                        await result
                is_first_connect = False

                symbols = register_set.all_symbols()
                if symbols:
                    try:
                        ok = await put_register(symbols)
                        if ok is False:
                            logger.warning(
                                "kabu put_register returned False for %d symbols",
                                len(symbols),
                            )
                    except KabuRateLimitError as put_exc:
                        # Rate-limit on replay resolves on its own — the next
                        # reconnect cycle will replay again.
                        logger.warning(
                            "kabu put_register rate-limited during replay, "
                            "will retry on next reconnect: %s", put_exc
                        )
                    # Any other put_register failure (token expired, register full,
                    # network) propagates to _run_ws → adapter.last_error →
                    # events(), making the dead session observable.

                while True:
                    try:
                        raw = await asyncio.wait_for(
                            ws.recv(), timeout=_RECV_TIMEOUT_S
                        )
                    except asyncio.TimeoutError:
                        logger.warning(
                            "kabu ws recv timeout after %.1fs, reconnecting",
                            _RECV_TIMEOUT_S,
                        )
                        break

                    consecutive_ok_close_count = 0
                    # 1 frame を正常受信した時点で session 健全と判断し failure を reset。
                    consecutive_failures = 0

                    if isinstance(raw, bytes):
                        text = raw.decode("utf-8")
                    else:
                        text = raw

                    try:
                        msg = json.loads(text)
                    except (ValueError, json.JSONDecodeError) as decode_exc:
                        logger.warning(
                            "kabu ws JSON decode error, dropping frame: %s",
                            decode_exc,
                        )
                        continue

                    try:
                        result = on_message(msg)
                        if asyncio.iscoroutine(result):
                            await result
                    except asyncio.CancelledError:
                        raise
                    except BaseException as cb_exc:
                        logger.warning(
                            "kabu ws on_message callback error: %s", cb_exc
                        )
                        continue

            # 内側 TimeoutError break 経路: 少し待って再接続。
            await asyncio.sleep(_RECONNECT_DELAY_S)

        except websockets.exceptions.ConnectionClosedOK as exc:
            consecutive_ok_close_count += 1
            if consecutive_ok_close_count > _MAX_RECONNECT_ATTEMPTS:
                raise KabuConnectionError(
                    f"repeated ConnectionClosedOK ({consecutive_ok_close_count} times)"
                ) from exc
            logger.info(
                "kabu ws ConnectionClosedOK (%d), reconnecting",
                consecutive_ok_close_count,
            )
            await asyncio.sleep(_RECONNECT_DELAY_S)

        except websockets.exceptions.ConnectionClosedError as exc:
            consecutive_failures += 1
            if consecutive_failures >= _MAX_RECONNECT_ATTEMPTS:
                raise KabuConnectionError(str(exc)) from exc
            logger.warning(
                "kabu ws ConnectionClosedError (%d/%d): %s",
                consecutive_failures,
                _MAX_RECONNECT_ATTEMPTS,
                exc,
            )
            await asyncio.sleep(_RECONNECT_DELAY_S)

        except (OSError, asyncio.TimeoutError) as exc:
            consecutive_failures += 1
            if consecutive_failures >= _MAX_RECONNECT_ATTEMPTS:
                raise KabuConnectionError(
                    f"WebSocket reconnect failed {consecutive_failures} times"
                ) from exc
            logger.warning(
                "kabu ws network error (%d/%d): %s",
                consecutive_failures,
                _MAX_RECONNECT_ATTEMPTS,
                exc,
            )
            await asyncio.sleep(_RECONNECT_DELAY_S)

