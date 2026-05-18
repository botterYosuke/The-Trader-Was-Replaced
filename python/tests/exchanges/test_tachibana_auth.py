import re

import pytest

from engine.exchanges.tachibana_auth import (
    ApiError,
    LoginError,
    SessionExpiredError,
    TachibanaError,
    UnreadNoticesError,
    build_params,
    check_response,
    current_p_sd_date,
    next_p_no,
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
