from __future__ import annotations

import asyncio
import itertools
import json
import logging
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from typing import Any, Awaitable, Mapping, MutableMapping, Optional

import httpx

from .tachibana_codec import decode_response_body
from .tachibana_url import (
    BASE_URL_DEMO,
    BASE_URL_PROD,
    EventUrl,
    MasterUrl,
    PriceUrl,
    RequestUrl,
    build_auth_url,
)

log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# User-facing banner text (F-Banner1 / architecture.md §6)
# ---------------------------------------------------------------------------
# Python composes the entire LoginError.message; Rust UI prints it verbatim.
# Server-side p_err / sResultText is intentionally NOT propagated to the user
# (it leaks Tachibana-internal English / inconsistent wording that breaks the
# Japanese banner contract) and is only logged for triage.

_MSG_LOGIN_FAILED = "ログインに失敗しました。ID / パスワードを確認してください"
_MSG_SERVICE_OUT_OF_HOURS = (
    "立花サーバーが現在サービス時間外です（デモ環境は平日 8:00–18:00 JST）。"
    "時間内に再ログインしてください"
)
_MSG_SESSION_EXPIRED_STARTUP = (
    "立花のセッションが切れました（夜間閉局）。再ログインしてください"
)
_MSG_TRANSPORT_ERROR = (
    "立花サーバとの通信に失敗しました。ネットワーク / プロキシ設定を確認してください"
)
_MSG_LOGIN_PARSE_FAILED = "立花ログイン応答の形式が不正です。サポートに連絡してください"
_MSG_VIRTUAL_URL_INVALID = (
    "立花ログイン応答の URL が想定と異なります。サポートに連絡してください"
)
_MSG_UNREAD_NOTICES = "未読の重要通知があります。e-shiten Web で確認後に再ログインしてください"

# p_errno codes indicating "server is currently outside service hours"
# rather than a credential problem. -62 = システムサービス時間外.
_SERVICE_OUT_OF_HOURS_CODES = frozenset({"-62"})

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
    "login",
    "validate_session_on_startup",
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

    def __init__(
        self,
        message: str = "login failed",
        *,
        code: str = "LOGIN_FAILED",
    ) -> None:
        super().__init__(message)
        self.message = message
        self.code = code


class UnreadNoticesError(TachibanaError):
    """Raised when the API reports unread notices (sKinsyouhouMidokuFlg=1)."""

    def __init__(
        self,
        message: str = "unread notices flag set",
        *,
        code: str = "UNREAD_NOTICES",
    ) -> None:
        super().__init__(message)
        self.message = message
        self.code = code


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

    def restore(self, value: int) -> None:
        if value > self._value:
            self._value = value

    def fast_forward(self, value: int) -> None:
        """Advance the counter past ``value`` so the next ``next()`` exceeds it.

        Unlike ``restore``, this guarantees forward progress even when the
        current ``_value`` is already greater than ``value`` (clock skew between
        the subprocess that consumed p_no and this counter's ``time.time()``
        seed). Required for the session_cache resume path (R4 monotonic).
        """
        self._value = max(self._value, value) + 1


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


# ---------------------------------------------------------------------------
# Phase 8 A1.3: login / validate_session_on_startup (stub for A1.3b RED tests)
# ---------------------------------------------------------------------------


def _validate_virtual_urls(payload: dict[str, Any]) -> None:
    """REST URLs must be https://, WS must be wss://.

    Virtual URLs are session-secret (they embed the ND= token), so errors
    route through mask_secrets() rather than logging the raw URL.
    """
    from engine.live.logging import mask_secrets

    for key in ("sUrlRequest", "sUrlMaster", "sUrlPrice", "sUrlEvent"):
        url = payload.get(key, "")
        if not isinstance(url, str) or not url.startswith("https://"):
            log.error(
                "tachibana login: %s did not start with https:// (masked=%r)",
                key,
                mask_secrets({key: url}),
            )
            raise LoginError(_MSG_VIRTUAL_URL_INVALID, code="login_failed")
    ws = payload.get("sUrlEventWebSocket", "")
    if not isinstance(ws, str) or not ws.startswith("wss://"):
        log.error(
            "tachibana login: sUrlEventWebSocket did not start with wss:// "
            "(masked=%r)",
            mask_secrets({"sUrlEventWebSocket": ws}),
        )
        raise LoginError(_MSG_VIRTUAL_URL_INVALID, code="login_failed")


def _decode_json(body: bytes) -> dict[str, Any]:
    text = decode_response_body(body)
    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        log.error("tachibana login: JSON parse failed: %s", exc)
        raise LoginError(_MSG_LOGIN_PARSE_FAILED, code="login_failed") from exc
    if not isinstance(data, dict):
        log.error(
            "tachibana login: response is not a JSON object (got %s)",
            type(data).__name__,
        )
        raise LoginError(_MSG_LOGIN_PARSE_FAILED, code="login_failed")
    return data


def _raise_for_login_error(data: Mapping[str, Any]) -> None:
    """Convert check_response() exceptions to login-banner-shaped exceptions.

    F-Banner1: Python composes the user-facing message; server-side
    p_err / sResultText is logged but never reaches the UI.
    """
    try:
        check_response(data)
    except SessionExpiredError as exc:
        raise SessionExpiredError(
            "session_expired", _MSG_SESSION_EXPIRED_STARTUP
        ) from exc
    except UnreadNoticesError as exc:
        raise UnreadNoticesError(
            _MSG_UNREAD_NOTICES, code="unread_notices"
        ) from exc
    except ApiError as exc:
        log.error(
            "tachibana login: API error code=%r server_message=%r",
            exc.code,
            exc.message,
        )
        if exc.code in _SERVICE_OUT_OF_HOURS_CODES:
            raise LoginError(
                _MSG_SERVICE_OUT_OF_HOURS, code=exc.code
            ) from exc
        raise LoginError(_MSG_LOGIN_FAILED, code=exc.code) from exc


async def _safe_get(client: httpx.AsyncClient, url: str) -> bytes:
    """GET url; map any HTTP / network failure to LoginError(transport_error).

    Without raise_for_status() a 502 / 503 / proxy HTML response would flow
    into _decode_json and surface as "JSON parse failed", burying the real
    transport problem.
    """
    try:
        resp = await client.get(url)
        resp.raise_for_status()
    except httpx.HTTPStatusError as exc:
        log.error(
            "tachibana login: HTTP %s from server (body prefix=%r)",
            exc.response.status_code,
            exc.response.content[:200],
        )
        raise LoginError(
            _MSG_TRANSPORT_ERROR, code="transport_error"
        ) from exc
    except httpx.HTTPError as exc:
        log.error("tachibana login: transport failure: %s", exc)
        raise LoginError(
            _MSG_TRANSPORT_ERROR, code="transport_error"
        ) from exc
    return resp.content


async def login(
    user_id: str,
    password: str,
    *,
    is_demo: bool,
    p_no_counter: PNoCounter,
    http_client: Optional[httpx.AsyncClient] = None,
) -> TachibanaSession:
    """Issue CLMAuthLoginRequest and return a TachibanaSession.

    p_no_counter is required so retries / startup re-login never reuse a
    p_no already accepted by the server (R4 monotonic contract).

    Raises:
        UnreadNoticesError: sKinsyouhouMidokuFlg=='1' (code='unread_notices').
        SessionExpiredError: p_errno=='2' (code='session_expired').
        LoginError: any other auth-time failure. code is one of
            the upstream p_errno / sResultCode string, '-62'
            (service hours), 'transport_error', or 'login_failed'.
    """
    base = BASE_URL_DEMO if is_demo else BASE_URL_PROD
    payload: dict[str, Any] = {
        "p_no": str(p_no_counter.next()),
        "p_sd_date": current_p_sd_date(),
        "sCLMID": "CLMAuthLoginRequest",
        "sUserId": user_id,
        "sPassword": password,
    }
    url = build_auth_url(base, payload, sJsonOfmt="5")

    own_client = http_client is None
    # Per-component timeouts: on Windows a scalar timeout does not bound
    # the connect phase when the virtual URL has expired (DNS resolves but
    # TCP SYN never gets a reply), causing a silent hang.
    _DEFAULT_TIMEOUT = httpx.Timeout(connect=10.0, read=30.0, write=10.0, pool=5.0)
    client = http_client or httpx.AsyncClient(timeout=_DEFAULT_TIMEOUT)
    try:
        body = await _safe_get(client, url)
    finally:
        if own_client:
            await client.aclose()

    data = _decode_json(body)
    _raise_for_login_error(data)
    _validate_virtual_urls(data)

    return TachibanaSession(
        url_request=RequestUrl(data["sUrlRequest"]),
        url_master=MasterUrl(data["sUrlMaster"]),
        url_price=PriceUrl(data["sUrlPrice"]),
        url_event=EventUrl(data["sUrlEvent"]),
        url_event_ws=data["sUrlEventWebSocket"],
        zyoutoeki_kazei_c=str(data.get("sZyoutoekiKazeiC", "")),
        expires_at_ms=None,
    )


async def validate_session_on_startup(*args: Any, **kwargs: Any) -> None:
    """Stub — real implementation lands in A1.4. Keyword-only at call site."""
    raise NotImplementedError(
        "validate_session_on_startup() is not yet implemented (A1.4)"
    )
