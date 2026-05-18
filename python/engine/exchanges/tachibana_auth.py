from __future__ import annotations

import time
from datetime import datetime, timedelta, timezone
from typing import Any, Mapping, MutableMapping, Optional

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

_p_no_counter: Optional[int] = None

_RESERVED_FIELDS = frozenset({"sCLMID", "sJsonOfmt", "p_no", "p_sd_date"})


def next_p_no() -> int:
    global _p_no_counter
    if _p_no_counter is None:
        _p_no_counter = int(time.time())
    else:
        _p_no_counter += 1
    return _p_no_counter


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
