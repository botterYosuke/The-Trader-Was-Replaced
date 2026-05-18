import pytest

from engine.exchanges.tachibana_url import (
    BASE_URL_DEMO,
    BASE_URL_PROD,
    base_url,
    build_event_url,
    build_request_url,
    func_replace_urlecnode,
)


def test_base_url_constants():
    assert BASE_URL_DEMO == "https://demo-kabuka.e-shiten.jp/e_api_v4r8/"
    assert BASE_URL_PROD == "https://kabuka.e-shiten.jp/e_api_v4r8/"


def test_base_url_demo():
    assert base_url("demo") == BASE_URL_DEMO


def test_base_url_prod_without_env(monkeypatch):
    monkeypatch.delenv("TACHIBANA_ALLOW_PROD", raising=False)
    with pytest.raises(RuntimeError):
        base_url("prod")


def test_base_url_prod_with_env(monkeypatch):
    monkeypatch.setenv("TACHIBANA_ALLOW_PROD", "1")
    assert base_url("prod") == BASE_URL_PROD


def test_base_url_invalid():
    with pytest.raises(ValueError):
        base_url("staging")  # type: ignore[arg-type]


@pytest.mark.parametrize(
    "ch,expected",
    [
        (" ", "%20"),
        ("!", "%21"),
        ('"', "%22"),
        ("%", "%25"),
        ("&", "%26"),
        ("+", "%2B"),
        ("/", "%2F"),
        ("=", "%3D"),
        ("?", "%3F"),
        ("{", "%7B"),
        ("}", "%7D"),
        ("~", "%7E"),
    ],
)
def test_func_replace_urlecnode_samples(ch, expected):
    assert func_replace_urlecnode(ch) == expected


def test_func_replace_urlecnode_percent_double_guard():
    assert func_replace_urlecnode("%20") == "%2520"


def test_build_request_url_starts_with_base_and_qmark():
    url = build_request_url(BASE_URL_DEMO, {"key": "value"})
    assert url.startswith(BASE_URL_DEMO + "?")


def test_build_event_url_preserves_order():
    url = build_event_url(BASE_URL_DEMO, {"p_rid": "1", "p_eno": "1"})
    assert url == BASE_URL_DEMO + "?p_rid=1&p_eno=1"
