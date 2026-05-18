"""kabusapi URL builder tests (kabu skill R1 / R4)."""

from __future__ import annotations

import pytest

from engine.exchanges.kabusapi_url import (
    BASE_URL_PROD,
    BASE_URL_VERIFY,
    base_url,
    endpoint,
    symbol_key,
)


def test_base_url_constants_importable():
    assert BASE_URL_PROD == "http://localhost:18080/kabusapi/"
    assert BASE_URL_VERIFY == "http://localhost:18081/kabusapi/"


def test_base_url_verify_default_allowed(monkeypatch):
    monkeypatch.delenv("KABU_ALLOW_PROD", raising=False)
    assert base_url("verify") == BASE_URL_VERIFY


def test_base_url_prod_requires_env_flag(monkeypatch):
    monkeypatch.delenv("KABU_ALLOW_PROD", raising=False)
    with pytest.raises(RuntimeError, match="KABU_ALLOW_PROD"):
        base_url("prod")


def test_base_url_prod_allowed_when_env_set(monkeypatch):
    monkeypatch.setenv("KABU_ALLOW_PROD", "1")
    assert base_url("prod") == BASE_URL_PROD


def test_base_url_unknown_env_raises():
    with pytest.raises(ValueError, match="invalid env"):
        base_url("staging")  # type: ignore[arg-type]


def test_endpoint_joins_base_and_path():
    assert endpoint("board/5401@1", env="verify") == BASE_URL_VERIFY + "board/5401@1"


def test_endpoint_strips_leading_slash():
    assert endpoint("/orders", env="verify") == BASE_URL_VERIFY + "orders"


def test_symbol_key_formats_symbol_and_exchange():
    assert symbol_key("5401", 1) == "5401@1"


def test_symbol_key_rejects_empty_symbol():
    with pytest.raises(ValueError, match="INVALID_SYMBOL"):
        symbol_key("", 1)


def test_ws_url_verify_returns_ws_scheme(monkeypatch):
    monkeypatch.delenv("KABU_ALLOW_PROD", raising=False)
    from engine.exchanges.kabusapi_url import ws_url

    assert ws_url("verify") == "ws://localhost:18081/kabusapi/websocket"


def test_ws_url_prod_requires_env_flag(monkeypatch):
    monkeypatch.delenv("KABU_ALLOW_PROD", raising=False)
    from engine.exchanges.kabusapi_url import ws_url

    with pytest.raises(RuntimeError, match="KABU_ALLOW_PROD"):
        ws_url("prod")


def test_ws_url_prod_allowed_when_env_set(monkeypatch):
    monkeypatch.setenv("KABU_ALLOW_PROD", "1")
    from engine.exchanges.kabusapi_url import ws_url

    assert ws_url("prod") == "ws://localhost:18080/kabusapi/websocket"


def test_kabu_env_alias_importable():
    """B4-2 で `kabusapi_ws.py` が `from ... import KabuEnv, ws_url` を要求する。"""
    from engine.exchanges.kabusapi_url import KabuEnv  # noqa: F401
