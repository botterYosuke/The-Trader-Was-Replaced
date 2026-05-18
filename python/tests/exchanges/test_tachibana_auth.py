import asyncio
import re
import time as _time

import pytest

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
    next_p_no,
)
from engine.exchanges.tachibana_url import (
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


def _make_session() -> TachibanaSession:
    return TachibanaSession(
        url_request=RequestUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/request/ND=/"),
        url_master=MasterUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/master/ND=/"),
        url_price=PriceUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/price/ND=/"),
        url_event=EventUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/event/ND=/"),
        url_event_ws="wss://demo-kabuka.e-shiten.jp/e_api_v4r8/event_ws/ND=/",
        zyoutoeki_kazei_c="1",
    )


def test_tachibana_session_holds_newtype_urls():
    s = _make_session()
    assert isinstance(s.url_request, RequestUrl)
    assert isinstance(s.url_master, MasterUrl)
    assert isinstance(s.url_price, PriceUrl)
    assert isinstance(s.url_event, EventUrl)


def test_tachibana_session_expires_at_ms_defaults_to_none():
    assert _make_session().expires_at_ms is None


def test_tachibana_session_is_frozen():
    s = _make_session()
    with pytest.raises((AttributeError, Exception)):
        s.zyoutoeki_kazei_c = "2"  # type: ignore[misc]


def test_tachibana_session_ws_url_uses_wss_scheme():
    assert _make_session().url_event_ws.startswith("wss://")


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
