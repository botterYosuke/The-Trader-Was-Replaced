"""TDD: tachibana_url module — REQUEST/AUTH/EVENT URL builders and percent-encoding."""

from __future__ import annotations

import json

import pytest

from engine.exchanges.tachibana_url import (
    AuthUrl,
    BASE_URL_DEMO,
    BASE_URL_PROD,
    EventUrl,
    MasterUrl,
    PriceUrl,
    RequestUrl,
    build_auth_url,
    build_event_url,
    build_request_url,
    func_replace_urlecnode,
)


def test_base_url_constants_are_auth_url_wrappers():
    assert isinstance(BASE_URL_DEMO, AuthUrl)
    assert isinstance(BASE_URL_PROD, AuthUrl)
    assert BASE_URL_DEMO.value == "https://demo-kabuka.e-shiten.jp/e_api_v4r8/"
    assert BASE_URL_PROD.value == "https://kabuka.e-shiten.jp/e_api_v4r8/"


def test_replace_urlecnode_each_target_char():
    table = {
        " ": "%20", "!": "%21", '"': "%22", "#": "%23", "$": "%24",
        "%": "%25", "&": "%26", "'": "%27", "(": "%28", ")": "%29",
        "*": "%2A", "+": "%2B", ",": "%2C", "/": "%2F", ":": "%3A",
        ";": "%3B", "<": "%3C", "=": "%3D", ">": "%3E", "?": "%3F",
        "@": "%40", "[": "%5B", "]": "%5D", "^": "%5E", "`": "%60",
        "{": "%7B", "|": "%7C", "}": "%7D", "~": "%7E",
    }
    for ch, encoded in table.items():
        assert func_replace_urlecnode(ch) == encoded, f"failed for {ch!r}"


def test_replace_urlecnode_empty():
    assert func_replace_urlecnode("") == ""


def test_replace_urlecnode_passthrough_alnum():
    assert func_replace_urlecnode("abcXYZ0189") == "abcXYZ0189"


def test_replace_urlecnode_full_roundtrip():
    from urllib.parse import unquote

    src = " !\"#$%&'()*+,/:;<=>?@[]^`{|}~ABC123"
    encoded = func_replace_urlecnode(src)
    assert unquote(encoded) == src


def test_replace_urlecnode_multibyte_shift_jis():
    out = func_replace_urlecnode("トヨタ自動車 7203")
    assert out == "トヨタ自動車%207203"


def test_build_request_url_requires_sJsonOfmt_kwarg():
    base = RequestUrl("https://example.invalid/v4r8/request/")
    with pytest.raises(TypeError):
        build_request_url(base, {"sCLMID": "X"})  # type: ignore[call-arg]


def test_build_request_url_rejects_unknown_sJsonOfmt():
    base = RequestUrl("https://example.invalid/v4r8/request/")
    with pytest.raises(ValueError):
        build_request_url(base, {"sCLMID": "X"}, sJsonOfmt="9")


def test_build_request_url_format_5():
    base = RequestUrl("https://example.invalid/v4r8/request/")
    url = build_request_url(base, {"sCLMID": "CLMOrderList", "p_no": "1"}, sJsonOfmt="5")
    assert url.startswith("https://example.invalid/v4r8/request/?")
    query = url.split("?", 1)[1]
    from urllib.parse import unquote

    obj = json.loads(unquote(query))
    assert obj["sJsonOfmt"] == "5"
    assert obj["sCLMID"] == "CLMOrderList"


def test_build_request_url_format_4_for_master_download():
    base = MasterUrl("https://example.invalid/v4r8/master/")
    url = build_request_url(base, {"sCLMID": "CLMEventDownload"}, sJsonOfmt="4")
    from urllib.parse import unquote

    obj = json.loads(unquote(url.split("?", 1)[1]))
    assert obj["sJsonOfmt"] == "4"


def test_build_request_url_rejects_event_url_type():
    bad = EventUrl("https://example.invalid/v4r8/event/")
    with pytest.raises(TypeError):
        build_request_url(bad, {"sCLMID": "X"}, sJsonOfmt="5")  # type: ignore[arg-type]


def test_build_request_url_accepts_price_url():
    base = PriceUrl("https://example.invalid/v4r8/price/")
    url = build_request_url(base, {"sCLMID": "CLMMfdsGetMarketPrice"}, sJsonOfmt="5")
    assert "CLMMfdsGetMarketPrice" in url


def test_build_request_url_rejects_control_chars_in_value():
    base = RequestUrl("https://example.invalid/v4r8/request/")
    for bad in ["\n", "\t", "\r", "\x01", "\x02", "\x03"]:
        with pytest.raises(ValueError):
            build_request_url(
                base, {"sCLMID": "X", "evil": f"a{bad}b"}, sJsonOfmt="5"
            )


def test_build_request_url_rejects_unsupported_value_types():
    base = RequestUrl("https://example.invalid/v4r8/request/")
    with pytest.raises(TypeError):
        build_request_url(base, {"sCLMID": "X", "evil": ["a", "b"]}, sJsonOfmt="5")
    with pytest.raises(TypeError):
        build_request_url(base, {"sCLMID": "X", "evil": {"k": "v"}}, sJsonOfmt="5")
    with pytest.raises(TypeError):
        build_request_url(base, {"sCLMID": "X", "evil": None}, sJsonOfmt="5")


def test_build_request_url_rejects_master_clmid_on_request_url():
    base = RequestUrl("https://example.invalid/v4r8/request/")
    with pytest.raises(TypeError, match="MasterUrl"):
        build_request_url(base, {"sCLMID": "CLMEventDownload"}, sJsonOfmt="4")


def test_build_request_url_rejects_price_clmid_on_master_url():
    base = MasterUrl("https://example.invalid/v4r8/master/")
    with pytest.raises(TypeError, match="PriceUrl"):
        build_request_url(base, {"sCLMID": "CLMMfdsGetMarketPrice"}, sJsonOfmt="5")


def test_build_auth_url_appends_auth_path_and_default_ofmt():
    url = build_auth_url(BASE_URL_DEMO, {"sCLMID": "CLMAuthLoginRequest", "sUserId": "u"})
    assert url.startswith("https://demo-kabuka.e-shiten.jp/e_api_v4r8/auth/?")
    from urllib.parse import unquote

    obj = json.loads(unquote(url.split("?", 1)[1]))
    assert obj["sJsonOfmt"] == "5"
    assert obj["sCLMID"] == "CLMAuthLoginRequest"
    assert obj["sUserId"] == "u"


def test_build_auth_url_rejects_non_auth_url_type():
    bad = RequestUrl("https://example.invalid/v4r8/request/")
    with pytest.raises(TypeError, match="AuthUrl"):
        build_auth_url(bad, {"sCLMID": "CLMAuthLoginRequest"})  # type: ignore[arg-type]


def test_build_auth_url_rejects_ofmt_4():
    with pytest.raises(ValueError):
        build_auth_url(BASE_URL_DEMO, {"sCLMID": "CLMAuthLoginRequest"}, sJsonOfmt="4")


def test_build_auth_url_rejects_control_chars():
    for bad in ["\n", "\t", "\r", "\x01"]:
        with pytest.raises(ValueError):
            build_auth_url(BASE_URL_DEMO, {"sUserId": f"a{bad}b"})


def test_build_event_url_keyvalue_form():
    base = EventUrl("https://example.invalid/v4r8/event/")
    url = build_event_url(
        base,
        {
            "p_evt_cmd": "FD,KP,ST",
            "p_eno": "0",
            "p_rid": "22",
            "p_board_no": "1000",
        },
    )
    assert url.startswith("https://example.invalid/v4r8/event/?")
    query = url.split("?", 1)[1]
    pairs = dict(p.split("=", 1) for p in query.split("&"))
    # Regression (e-station bug-postmortem 2026-05-01): commas in p_evt_cmd MUST
    # stay raw. The server does not percent-decode EVENT params, so '%2C' silently
    # drops the subscription. The REQUEST-style func_replace_urlecnode must NOT run.
    assert pairs["p_evt_cmd"] == "FD,KP,ST"
    assert pairs["p_eno"] == "0"
    assert pairs["p_rid"] == "22"


def test_build_event_url_evt_cmd_has_raw_commas():
    """p_evt_cmd list separators are literal commas, never %2C (raw-comma guard)."""
    base = EventUrl("https://example.invalid/v4r8/event/")
    url = build_event_url(base, {"p_evt_cmd": "ST,KP,EC,SS,US"})
    assert "p_evt_cmd=ST,KP,EC,SS,US" in url
    assert "%2C" not in url


def test_build_event_url_rejects_request_url_type():
    bad = RequestUrl("https://example.invalid/v4r8/request/")
    with pytest.raises(TypeError):
        build_event_url(bad, {"p_eno": "0"})  # type: ignore[arg-type]


def test_build_event_url_rejects_control_chars():
    base = EventUrl("https://example.invalid/v4r8/event/")
    for bad in ["\n", "\t", "\r", "\x01", "\x02", "\x03"]:
        with pytest.raises(ValueError):
            build_event_url(base, {"p_evt_cmd": f"FD{bad}KP"})


def test_build_event_url_rejects_structure_injection_in_value():
    """Raw values must not be able to smuggle URL-structure chars / extra params."""
    base = EventUrl("https://example.invalid/v4r8/event/")
    for bad in ["FD&p_x=1", "FD=KP", "FD?KP", "FD%2CKP", "FD KP", "7203.T"]:
        with pytest.raises(ValueError):
            build_event_url(base, {"p_evt_cmd": bad})


def test_build_event_url_rejects_bad_param_name():
    base = EventUrl("https://example.invalid/v4r8/event/")
    with pytest.raises(ValueError):
        build_event_url(base, {"p evt": "FD"})


def test_build_event_url_rejects_degenerate_evt_cmd():
    """Empty / comma-only / double-comma p_evt_cmd would silently subscribe to
    nothing — the exact failure the raw-comma boundary exists to prevent. They
    must fail loud, not pass (review M1)."""
    base = EventUrl("https://example.invalid/v4r8/event/")
    for bad in ["", ",", "ST,", ",KP", "ST,,KP"]:
        with pytest.raises(ValueError):
            build_event_url(base, {"p_evt_cmd": bad})


def test_build_event_url_accepts_valid_token_lists():
    """Single tokens and comma-joined non-empty tokens stay valid (regression
    guard so the tightened pattern didn't over-reject real params)."""
    base = EventUrl("https://example.invalid/v4r8/event/")
    url = build_event_url(
        base,
        {"p_evt_cmd": "ST,KP,FD", "p_eno": "0", "p_mkt_code": "00",
         "p_issue_code": "130A", "p_board_no": "1000"},
    )
    assert "p_evt_cmd=ST,KP,FD" in url
    assert "p_mkt_code=00" in url
    assert "p_issue_code=130A" in url
