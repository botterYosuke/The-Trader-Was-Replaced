import asyncio
import json
import re
import time as _time
from urllib.parse import unquote

import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.tachibana_auth import (
    ApiError,
    LoginError,
    PNoCounter,
    SessionExpiredError,
    StartupLatch,
    TachibanaError,
    TachibanaSession,
    UnreadNoticesError,
    build_params,
    check_response,
    current_p_sd_date,
    login,
    next_p_no,
    validate_session_on_startup,
)
from engine.exchanges.tachibana_url import (
    BASE_URL_DEMO,
    EventUrl,
    MasterUrl,
    PriceUrl,
    RequestUrl,
)


# ---------- next_p_no ----------

def test_next_p_no_monotonic():
    a = next_p_no()
    b = next_p_no()
    c = next_p_no()
    assert b == a + 1
    assert c == b + 1


def test_next_p_no_returns_int():
    assert isinstance(next_p_no(), int)


# ---------- current_p_sd_date ----------

def test_current_p_sd_date_format():
    s = current_p_sd_date()
    assert re.fullmatch(r"\d{4}\.\d{2}\.\d{2}-\d{2}:\d{2}:\d{2}\.\d{3}", s), s


# ---------- build_params ----------

def test_build_params_includes_required_fields():
    p = build_params("CLMOrderList")
    assert p["sCLMID"] == "CLMOrderList"
    assert p["sJsonOfmt"] == "5"
    assert "p_no" in p
    assert "p_sd_date" in p
    assert isinstance(p["p_no"], int)
    assert re.fullmatch(r"\d{4}\.\d{2}\.\d{2}-\d{2}:\d{2}:\d{2}\.\d{3}", p["p_sd_date"])


def test_build_params_event_download_uses_ofmt_4():
    p = build_params("CLMEventDownload")
    assert p["sJsonOfmt"] == "4"
    assert p["sCLMID"] == "CLMEventDownload"


def test_build_params_extra_merges():
    p = build_params("CLMOrderList", extra={"sIssueCode": "7203"})
    assert p["sIssueCode"] == "7203"
    assert p["sCLMID"] == "CLMOrderList"
    assert p["sJsonOfmt"] == "5"


def test_build_params_extra_cannot_override_reserved():
    with pytest.raises(ValueError):
        build_params("CLMOrderList", extra={"sJsonOfmt": "1"})
    with pytest.raises(ValueError):
        build_params("CLMOrderList", extra={"sCLMID": "Other"})


def test_build_params_p_no_increments_between_calls():
    p1 = build_params("CLMOrderList")
    p2 = build_params("CLMOrderList")
    assert p2["p_no"] == p1["p_no"] + 1


# ---------- check_response ----------

def test_check_response_success_with_zero_strings():
    check_response({"p_errno": "0", "sResultCode": "0"})


def test_check_response_success_with_empty_p_errno():
    check_response({"p_errno": "", "sResultCode": "0"})


def test_check_response_p_errno_nonzero_raises_api_error():
    with pytest.raises(ApiError) as ei:
        check_response({"p_errno": "1", "sResultCode": "0"})
    assert ei.value.code == "1"


def test_check_response_p_errno_2_raises_session_expired():
    with pytest.raises(SessionExpiredError):
        check_response({"p_errno": "2", "sResultCode": "0"})


def test_check_response_sresultcode_nonzero_raises_api_error():
    with pytest.raises(ApiError) as ei:
        check_response({"p_errno": "0", "sResultCode": "9"})
    assert ei.value.code == "9"


def test_check_response_unread_notices_flag_raises():
    with pytest.raises(UnreadNoticesError):
        check_response(
            {"p_errno": "0", "sResultCode": "0", "sKinsyouhouMidokuFlg": "1"}
        )


def test_check_response_unread_notices_flag_zero_is_ok():
    check_response(
        {"p_errno": "0", "sResultCode": "0", "sKinsyouhouMidokuFlg": "0"}
    )


# ---------- 例外階層 ----------

def test_exception_hierarchy():
    assert issubclass(ApiError, TachibanaError)
    assert issubclass(LoginError, TachibanaError)
    assert issubclass(UnreadNoticesError, TachibanaError)
    assert issubclass(SessionExpiredError, TachibanaError)


# ---------- PNoCounter (A1.2) ----------


def test_p_no_counter_first_next_is_unix_seconds_plus_one():
    before = int(_time.time())
    c = PNoCounter()
    v = c.next()
    after = int(_time.time())
    assert before + 1 <= v <= after + 1


def test_p_no_counter_monotonic_within_instance():
    c = PNoCounter()
    a = c.next()
    b = c.next()
    d = c.next()
    assert b == a + 1
    assert d == b + 1


def test_p_no_counter_instances_are_independent():
    c1 = PNoCounter()
    c2 = PNoCounter()
    c1.next()
    c1.next()
    c1.next()
    assert c2.peek() == c2.peek()
    v2 = c2.next()
    assert v2 == c2.peek()


def test_p_no_counter_peek_does_not_advance():
    c = PNoCounter()
    c.next()
    p1 = c.peek()
    p2 = c.peek()
    assert p1 == p2


def test_p_no_counter_next_returns_int():
    assert isinstance(PNoCounter().next(), int)


# ---------- TachibanaSession (A1.2) ----------


def _make_login_session() -> TachibanaSession:
    return TachibanaSession(
        url_request=RequestUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/request/ND=/"),
        url_master=MasterUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/master/ND=/"),
        url_price=PriceUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/price/ND=/"),
        url_event=EventUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/event/ND=/"),
        url_event_ws="wss://demo-kabuka.e-shiten.jp/e_api_v4r8/event_ws/ND=/",
        zyoutoeki_kazei_c="1",
    )


def test_tachibana_session_holds_newtype_urls():
    s = _make_login_session()
    assert isinstance(s.url_request, RequestUrl)
    assert isinstance(s.url_master, MasterUrl)
    assert isinstance(s.url_price, PriceUrl)
    assert isinstance(s.url_event, EventUrl)


def test_tachibana_session_expires_at_ms_defaults_to_none():
    assert _make_login_session().expires_at_ms is None


def test_tachibana_session_is_frozen():
    s = _make_login_session()
    with pytest.raises((AttributeError, Exception)):
        s.zyoutoeki_kazei_c = "2"  # type: ignore[misc]


def test_tachibana_session_ws_url_uses_wss_scheme():
    assert _make_login_session().url_event_ws.startswith("wss://")


# ---------- StartupLatch (M3) ----------


async def test_startup_latch_first_call_returns_coro_result():
    latch = StartupLatch()

    async def coro_ok():
        return 42

    assert await latch.run_once(coro_ok()) == 42


async def test_startup_latch_second_call_raises_runtime_error():
    latch = StartupLatch()

    async def coro_ok():
        return 42

    await latch.run_once(coro_ok())
    with pytest.raises(RuntimeError):
        await latch.run_once(coro_ok())


async def test_startup_latch_second_call_after_failure_still_raises():
    latch = StartupLatch()

    async def coro_fail():
        raise ValueError("boom")

    with pytest.raises(ValueError):
        await latch.run_once(coro_fail())
    with pytest.raises(RuntimeError):
        await latch.run_once(coro_fail())


async def test_startup_latch_concurrent_calls_exactly_one_succeeds():
    latch = StartupLatch()

    async def coro():
        await asyncio.sleep(0)
        return "ok"

    results = await asyncio.gather(
        latch.run_once(coro()),
        latch.run_once(coro()),
        return_exceptions=True,
    )
    runtime_errors = [r for r in results if isinstance(r, RuntimeError)]
    successes = [r for r in results if r == "ok"]
    assert len(runtime_errors) == 1
    assert len(successes) == 1


# ===========================================================================
# A1.3b login() RED tests (写経 from e-station)
# ===========================================================================
#
# These exercise `login()` / `validate_session_on_startup()` which are still
# NotImplementedError stubs. RED 観測 = 13 件 FAILED (NotImplementedError) を
# A1.3b 完走条件とし、A1.4 実装で GREEN に転じる。
#
# Test URLs are derived from BASE_URL_DEMO so the demo host literal stays
# single-sourced in `tachibana_url.py` (F-L1 / T7 secret_scan allowlist).

_DEMO_BASE = BASE_URL_DEMO.value  # ends with "/"
_DEMO_HOST_PATH = _DEMO_BASE.removeprefix("https://").removesuffix("/")

_AUTH_URL_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}auth/\?")
_VIRTUAL_REQUEST = f"{_DEMO_BASE}request/ND=/"
_VIRTUAL_MASTER = f"{_DEMO_BASE}master/ND=/"
_VIRTUAL_PRICE = f"{_DEMO_BASE}price/ND=/"
_VIRTUAL_EVENT = f"{_DEMO_BASE}event/ND=/"
_VIRTUAL_EVENT_WS = f"wss://{_DEMO_HOST_PATH}/event_ws/ND=/"


def _ok_login_payload(**overrides) -> dict:
    base = {
        "p_no": "1",
        "p_sd_date": "2026.04.25-10:00:00.000",
        "p_errno": "0",
        "p_err": "",
        "sCLMID": "CLMAuthLoginAck",
        "sResultCode": "0",
        "sResultText": "",
        "sZyoutoekiKazeiC": "1",
        "sKinsyouhouMidokuFlg": "0",
        "sUrlRequest": _VIRTUAL_REQUEST,
        "sUrlMaster": _VIRTUAL_MASTER,
        "sUrlPrice": _VIRTUAL_PRICE,
        "sUrlEvent": _VIRTUAL_EVENT,
        "sUrlEventWebSocket": _VIRTUAL_EVENT_WS,
    }
    base.update(overrides)
    return base


def _add_login_response(httpx_mock: HTTPXMock, payload: dict | str) -> None:
    """Tachibana returns Shift-JIS bytes; emulate that here."""
    body = payload if isinstance(payload, str) else json.dumps(payload, ensure_ascii=False)
    httpx_mock.add_response(
        url=_AUTH_URL_RE,
        method="GET",
        content=body.encode("shift_jis"),
    )


def _decode_query_json(url: str) -> dict:
    """Recover the JSON object from the bespoke percent-encoded query."""
    _, _, q = url.partition("?")
    return json.loads(unquote(q))


# ---------- Happy path ----------


async def test_login_returns_session_on_success(httpx_mock: HTTPXMock):
    _add_login_response(httpx_mock, _ok_login_payload())
    session = await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert isinstance(session, TachibanaSession)
    assert isinstance(session.url_master, MasterUrl)
    assert session.url_event_ws.startswith("wss://")
    assert session.zyoutoeki_kazei_c == "1"
    assert session.expires_at_ms is None  # F-B3


async def test_login_request_uses_json_ofmt_five(httpx_mock: HTTPXMock):
    """MEDIUM-C3-1 — auth endpoint requires sJsonOfmt='5'."""
    _add_login_response(httpx_mock, _ok_login_payload())
    await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    request = httpx_mock.get_request()
    assert request is not None
    query = _decode_query_json(str(request.url))
    assert query["sJsonOfmt"] == "5"
    assert query["sCLMID"] == "CLMAuthLoginRequest"
    assert query["sUserId"] == "uid"
    assert query["sPassword"] == "pwd"


async def test_login_consumes_p_no_counter_so_retries_are_monotonic(
    httpx_mock: HTTPXMock,
):
    """R4 — two `login()` calls on the same counter must send strictly
    increasing `p_no` values, so a startup retry never replays the prior
    request id."""
    _add_login_response(httpx_mock, _ok_login_payload())
    _add_login_response(httpx_mock, _ok_login_payload())
    counter = PNoCounter()
    await login("uid", "pwd", is_demo=True, p_no_counter=counter)
    await login("uid", "pwd", is_demo=True, p_no_counter=counter)
    requests = httpx_mock.get_requests()
    p_nos = [int(_decode_query_json(str(r.url))["p_no"]) for r in requests]
    assert p_nos[1] > p_nos[0]


# ---------- URL scheme validation (MEDIUM-C3-3) ----------


async def test_login_rejects_non_wss_event_url(httpx_mock: HTTPXMock):
    payload = _ok_login_payload(
        sUrlEventWebSocket=_VIRTUAL_EVENT_WS.replace("wss://", "ws://", 1),
    )
    _add_login_response(httpx_mock, payload)
    with pytest.raises(LoginError):
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())


async def test_login_rejects_non_https_request_url(httpx_mock: HTTPXMock):
    payload = _ok_login_payload(
        sUrlRequest=_VIRTUAL_REQUEST.replace("https://", "http://", 1),
    )
    _add_login_response(httpx_mock, payload)
    with pytest.raises(LoginError):
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())


# ---------- Error mapping (R6 / two-stage check + sKinsyouhouMidokuFlg) ----------


async def test_login_raises_unread_notices_when_kinsyouhou_flag_set(httpx_mock: HTTPXMock):
    """HIGH-C2-1: sKinsyouhouMidokuFlg='1' → UnreadNoticesError → unread_notices."""
    _add_login_response(
        httpx_mock,
        _ok_login_payload(sKinsyouhouMidokuFlg="1"),
    )
    with pytest.raises(UnreadNoticesError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert exc_info.value.code == "unread_notices"


async def test_session_expired_p_errno_2(httpx_mock: HTTPXMock):
    _add_login_response(
        httpx_mock,
        _ok_login_payload(p_errno="2", p_err="session expired"),
    )
    with pytest.raises(SessionExpiredError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert exc_info.value.code == "session_expired"


async def test_login_p_errno_minus_62_raises_login_error(httpx_mock: HTTPXMock):
    """Generic non-2 `p_errno` on the auth path must be bucketed as
    `LoginError` (login_path=True), not surface as a bare `TachibanaError`."""
    _add_login_response(
        httpx_mock,
        _ok_login_payload(p_errno="-62", p_err="auth-rate"),
    )
    with pytest.raises(LoginError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert exc_info.value.code == "-62"


async def test_login_p_errno_minus_62_uses_service_out_of_hours_banner(
    httpx_mock: HTTPXMock,
):
    """`p_errno=-62` ("システムサービス時間外") MUST surface a dedicated banner
    instead of the misleading credential-check string. Raw `p_err` must not
    leak (F-Banner1)."""
    _add_login_response(
        httpx_mock,
        _ok_login_payload(p_errno="-62", p_err="システムサービス時間外。"),
    )
    with pytest.raises(LoginError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    msg = exc_info.value.message
    assert "サービス時間外" in msg
    assert "ID" not in msg and "パスワード" not in msg, (
        "service-hours banner must NOT recycle the credential-check wording"
    )
    assert "システムサービス時間外。" not in msg, (
        "raw server p_err text must not leak into the banner (F-Banner1)"
    )


async def test_login_authentication_failure_raises_login_error(httpx_mock: HTTPXMock):
    """Same guarantee for `sResultCode != "0"` on the auth path."""
    _add_login_response(
        httpx_mock,
        _ok_login_payload(
            sResultCode="10031",
            sResultText="invalid credentials",
        ),
    )
    with pytest.raises(LoginError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert exc_info.value.code == "10031"


# ---------- Banner-text contract (F-Banner1) ----------


async def test_login_failure_message_uses_fixed_japanese_banner(
    httpx_mock: HTTPXMock,
):
    _add_login_response(
        httpx_mock,
        _ok_login_payload(
            sResultCode="10031",
            sResultText="invalid credentials",
        ),
    )
    with pytest.raises(LoginError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert "invalid credentials" not in exc_info.value.message
    assert exc_info.value.message == (
        "ログインに失敗しました。ID / パスワードを確認してください"
    )


async def test_session_expired_message_is_python_composed(httpx_mock: HTTPXMock):
    _add_login_response(
        httpx_mock,
        _ok_login_payload(p_errno="2", p_err="session expired"),
    )
    with pytest.raises(SessionExpiredError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert "session expired" not in exc_info.value.message
    assert exc_info.value.message == (
        "立花のセッションが切れました（夜間閉局）。再ログインしてください"
    )


# ---------- HTTP / transport error mapping ----------


async def test_login_http_502_maps_to_transport_error(httpx_mock: HTTPXMock):
    """5xx must surface as transport_error, not as a JSON parse failure."""
    httpx_mock.add_response(
        url=_AUTH_URL_RE,
        method="GET",
        status_code=502,
        content=b"<html>Bad Gateway</html>",
    )
    with pytest.raises(LoginError) as exc_info:
        await login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())
    assert exc_info.value.code == "transport_error"


# ---------------------------------------------------------------------------
# Post-merge review fix MEDIUM-1: _validate_virtual_urls must mask URLs in logs
# ---------------------------------------------------------------------------


async def test_validate_virtual_urls_does_not_leak_url_in_log(httpx_mock, caplog):
    """R3/R10 — invalid virtual URL must be reported with mask_secrets,
    not raw %r (which leaks the secret virtual session URL).
    """
    leaky_url = "http://evil-leak.example/request/ND=SECRETTOKEN/"
    payload = {
        "p_no": "1",
        "p_sd_date": "2026.04.25-10:00:00.000",
        "p_errno": "0",
        "sResultCode": "0",
        "sKinsyouhouMidokuFlg": "0",
        "sUrlRequest": leaky_url,  # http:// not https:// → triggers validation error
        "sUrlMaster": "https://x/m/",
        "sUrlPrice": "https://x/p/",
        "sUrlEvent": "https://x/e/",
        "sUrlEventWebSocket": "wss://x/ws/",
    }
    auth_url_re = re.compile(r"^https://demo-kabuka\.e-shiten\.jp/e_api_v4r8/auth/\?")
    httpx_mock.add_response(
        url=auth_url_re, method="GET",
        content=json.dumps(payload, ensure_ascii=False).encode("shift_jis"),
    )
    from engine.exchanges.tachibana_auth import login as _login

    with caplog.at_level("ERROR"):
        with pytest.raises(LoginError):
            await _login("uid", "pwd", is_demo=True, p_no_counter=PNoCounter())

    # The raw URL (in particular the SECRETTOKEN session segment) must NOT
    # appear anywhere in the captured log records.
    joined = "\n".join(r.getMessage() for r in caplog.records)
    assert "SECRETTOKEN" not in joined
    assert leaky_url not in joined
