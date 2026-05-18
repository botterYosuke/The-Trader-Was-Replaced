"""kabu STATION PUSH WebSocket connection manager.

Responsibility:
- websockets.connect(url, ping_interval=None, compression=None) で接続
  (kabuStation は RFC6455 非準拠 PONG + permessage-deflate RSV1 バグがあるため
   keepalive と圧縮を無効化する)
- 接続直後に register_set の銘柄を put_register で再送
- ws.recv() ループで JSON frame を on_message に dispatch
- OSError / ConnectionClosedError は consecutive_failures を増やし、
  >= _MAX_RECONNECT_ATTEMPTS で KabuConnectionError raise
- ConnectionClosedOK は consecutive_ok_close_count を増やし、
  > _MAX_RECONNECT_ATTEMPTS で KabuConnectionError raise
- それ以外の Exception も failures カウント側で同様
- 各 except 後は _RECONNECT_DELAY_S sleep して再接続

asyncio.CancelledError は BaseException なので except Exception で
捕まらずそのまま伝播する (caller の停止経路を阻害しない).
"""
from __future__ import annotations

import asyncio
import json
import logging
from typing import Awaitable, Callable

import websockets
import websockets.exceptions

from engine.exchanges.kabusapi_auth import KabuConnectionError
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
) -> None:
    """Manage a kabu PUSH WebSocket session with auto-reconnect.

    Raises KabuConnectionError when reconnect attempts exceed
    _MAX_RECONNECT_ATTEMPTS. Returns only via cancellation / caller-raised
    BaseException (e.g. on_message raising CancelledError).
    """
    url = ws_url(env)
    consecutive_failures = 0
    consecutive_ok_close_count = 0

    while True:
        try:
            async with websockets.connect(
                url,
                ping_interval=None,
                compression=None,
            ) as ws:
                symbols = register_set.all_symbols()
                if symbols:
                    ok = await put_register(symbols)
                    if not ok:
                        logger.warning(
                            "kabu put_register returned False for %d symbols", len(symbols)
                        )

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
                    # `async with` 直後にリセットすると、recv 即 ConnectionClosedError を
                    # 繰り返す病的シナリオで上限到達せず無限ループになる。
                    consecutive_failures = 0

                    if isinstance(raw, bytes):
                        text = raw.decode("utf-8")
                    else:
                        text = raw

                    msg = json.loads(text)
                    result = on_message(msg)
                    if asyncio.iscoroutine(result):
                        await result

            # 内側 TimeoutError break 経路: 少し待って再接続。
            # `async with` の __aexit__ が例外を投げた場合はここに到達せず、
            # 外側 except 側 (各々が個別に await asyncio.sleep) に分岐する。
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

        except OSError as exc:
            consecutive_failures += 1
            if consecutive_failures >= _MAX_RECONNECT_ATTEMPTS:
                raise KabuConnectionError(
                    f"WebSocket reconnect failed {consecutive_failures} times"
                ) from exc
            logger.warning(
                "kabu ws OSError (%d/%d): %s",
                consecutive_failures,
                _MAX_RECONNECT_ATTEMPTS,
                exc,
            )
            await asyncio.sleep(_RECONNECT_DELAY_S)

        except Exception as exc:
            consecutive_failures += 1
            if consecutive_failures >= _MAX_RECONNECT_ATTEMPTS:
                raise KabuConnectionError(str(exc)) from exc
            logger.warning(
                "kabu ws unexpected error (%d/%d): %s",
                consecutive_failures,
                _MAX_RECONNECT_ATTEMPTS,
                exc,
            )
            await asyncio.sleep(_RECONNECT_DELAY_S)
