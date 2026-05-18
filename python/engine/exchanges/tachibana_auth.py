from __future__ import annotations

import asyncio
import itertools
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from typing import Any, Awaitable, Mapping, MutableMapping, Optional

from .tachibana_url import EventUrl, MasterUrl, PriceUrl, RequestUrl

__all__ = [
    "TachibanaError",
    "ApiError",
    "LoginError",
    "UnreadNoticesError",
    "SessionExpiredError",
    "next_p_no",
    "current_p_sd_date",
    "build_params",
    "check_response",
]


class TachibanaError(Exception):
    """Base exception for Tachibana API errors."""


class ApiError(TachibanaError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


class LoginError(TachibanaError):
    """Raised when login fails."""


class UnreadNoticesError(TachibanaError):
    """Raised when the API reports unread notices (sKinsyouhouMidokuFlg=1)."""


class SessionExpiredError(ApiError):
    """Raised when the session has expired (p_errno=2)."""


_JST = timezone(timedelta(hours=9))

_p_no_lock = threading.Lock()
_p_no_iter: Optional[itertools.count] = None

_RESERVED_FIELDS = frozenset({"sCLMID", "sJsonOfmt", "p_no", "p_sd_date"})


def next_p_no() -> int:
    """Return the next monotonically increasing p_no.

    Tachibana サーバは p_no が逆転すると `code=6` を返すため、
    複数スレッド / リトライから同時に呼ばれても重複・逆転しないよう Lock で守る。
    """
    global _p_no_iter
    with _p_no_lock:
        if _p_no_iter is None:
            _p_no_iter = itertools.count(int(time.time()))
        return next(_p_no_iter)


def current_p_sd_date() -> str:
    now = datetime.now(_JST)
    ms = now.microsecond // 1000
    return f"{now.year:04d}.{now.month:02d}.{now.day:02d}-{now.hour:02d}:{now.minute:02d}:{now.second:02d}.{ms:03d}"


def build_params(
    s_clmid: str,
    *,
    extra: Optional[Mapping[str, Any]] = None,
) -> dict[str, Any]:
    if extra:
        for key in extra:
            if key in _RESERVED_FIELDS:
                raise ValueError(f"reserved field cannot be overridden: {key}")

    s_json_ofmt = "4" if s_clmid == "CLMEventDownload" else "5"

    params: dict[str, Any] = {
        "sCLMID": s_clmid,
        "sJsonOfmt": s_json_ofmt,
        "p_no": next_p_no(),
        "p_sd_date": current_p_sd_date(),
    }
    if extra:
        params.update(extra)
    return params


def check_response(payload: Mapping[str, Any]) -> None:
    p_errno = payload.get("p_errno", "")
    if p_errno == "2":
        raise SessionExpiredError("2", "session expired")
    if p_errno not in ("", "0"):
        raise ApiError(p_errno, f"p_errno={p_errno}")

    s_result_code = payload.get("sResultCode", "0")
    if s_result_code != "0":
        raise ApiError(s_result_code, f"sResultCode={s_result_code}")

    midoku = payload.get("sKinsyouhouMidokuFlg", "0")
    if midoku == "1":
        raise UnreadNoticesError("unread notices flag set")


# ---------------------------------------------------------------------------
# Phase 8 A1: PNoCounter / TachibanaSession / StartupLatch
# ---------------------------------------------------------------------------


class PNoCounter:
    """Per-instance monotonic `p_no` generator."""

    __slots__ = ("_value",)

    def __init__(self) -> None:
        self._value = int(time.time())

    def next(self) -> int:
        self._value += 1
        return self._value

    def peek(self) -> int:
        return self._value


@dataclass(frozen=True, slots=True)
class TachibanaSession:
    """Result of a successful login."""

    url_request: RequestUrl
    url_master: MasterUrl
    url_price: PriceUrl
    url_event: EventUrl
    url_event_ws: str
    zyoutoeki_kazei_c: str
    expires_at_ms: Optional[int] = None


class StartupLatch:
    """Allow `validate_session_on_startup` exactly once per instance."""

    __slots__ = ("_lock", "_done")

    def __init__(self) -> None:
        self._lock = asyncio.Lock()
        self._done = False

    async def run_once(self, coro: "Awaitable[Any]") -> "Any":
        async with self._lock:
            if self._done:
                if asyncio.iscoroutine(coro):
                    coro.close()
                raise RuntimeError(
                    "validate_session_on_startup は 1 プロセスライフサイクル中に "
                    "1 度だけ呼べる (L6)。"
                )
            try:
                return await coro
            finally:
                self._done = True
