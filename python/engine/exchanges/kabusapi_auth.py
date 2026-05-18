"""kabu STATION API auth & error helpers.

Implements:
- exception hierarchy (KabuError / KabuApiError / KabuTokenExpiredError /
  KabuRateLimitError / KabuConnectionError)
- check_response(payload, http_status): two-stage HTTP + Code judgement (R7)
- auth_headers(token): build X-API-KEY header (R3)

NOTE: fetch_token() is intentionally deferred to the HTTP client step
later in Phase 8 (requires HTTPXMock-based tests).
"""


class KabuError(Exception):
    """Base class for all kabu STATION API failures."""


class KabuApiError(KabuError):
    def __init__(self, code: int | str, message: str) -> None:
        super().__init__(f"[{code}] {message}")
        self.code = code
        self.message = message


class KabuTokenExpiredError(KabuApiError):
    """Code 4001005 — token expired, caller must re-auth."""


class KabuRateLimitError(KabuApiError):
    """Code 4002006 — rate limit exceeded."""


class KabuConnectionError(KabuError):
    """kabu STATION body process not reachable (connection refused etc.)."""


def check_response(payload: dict, http_status: int) -> None:
    """Two-stage response validation per kabu skill R7.

    1. HTTP >= 400 → KabuApiError (attach Code/Message if present in payload).
    2. HTTP 2xx but payload Code != 0 → specialized subclass for known codes,
       otherwise generic KabuApiError.
    """
    if http_status >= 400:
        code = payload.get("Code", http_status) if isinstance(payload, dict) else http_status
        message = payload.get("Message", f"HTTP {http_status}") if isinstance(payload, dict) else f"HTTP {http_status}"
        raise KabuApiError(code, message)

    if not isinstance(payload, dict):
        return

    code = payload.get("Code", 0)
    if code == 0:
        return

    message = payload.get("Message", "")
    if code == 4001005:
        raise KabuTokenExpiredError(code, message)
    if code == 4002006:
        raise KabuRateLimitError(code, message)
    raise KabuApiError(code, message)


def auth_headers(token: str) -> dict[str, str]:
    """Build kabu STATION auth header per R3.

    Raises ValueError if token is empty.
    """
    if not token:
        raise ValueError("token must be non-empty")
    return {"X-API-KEY": token}
