"""Tests for kabusapi_auth check_response / auth_headers / exception hierarchy."""

import pytest

from engine.exchanges.kabusapi_auth import (
    KabuApiError,
    KabuConnectionError,
    KabuError,
    KabuRateLimitError,
    KabuTokenExpiredError,
    auth_headers,
    check_response,
)

import httpx
from pytest_httpx import HTTPXMock

from engine.exchanges.kabusapi_url import endpoint


class TestExceptionHierarchy:
    def test_api_error_is_kabu_error(self):
        assert issubclass(KabuApiError, KabuError)

    def test_token_expired_is_api_error(self):
        assert issubclass(KabuTokenExpiredError, KabuApiError)

    def test_rate_limit_is_api_error(self):
        assert issubclass(KabuRateLimitError, KabuApiError)

    def test_connection_error_is_kabu_error(self):
        assert issubclass(KabuConnectionError, KabuError)
        assert not issubclass(KabuConnectionError, KabuApiError)


class TestCheckResponse:
    def test_ok_does_nothing(self):
        check_response({"Code": 0, "Message": ""}, 200)

    def test_http_401_raises_api_error(self):
        with pytest.raises(KabuApiError) as exc:
            check_response({"Code": 4001001, "Message": "Unauthorized"}, 401)
        assert exc.value.code == 4001001

    def test_http_500_raises_api_error(self):
        with pytest.raises(KabuApiError):
            check_response({}, 500)

    def test_code_4001005_raises_token_expired(self):
        with pytest.raises(KabuTokenExpiredError) as exc:
            check_response({"Code": 4001005, "Message": "token expired"}, 200)
        assert exc.value.code == 4001005

    def test_code_4002006_raises_rate_limit(self):
        with pytest.raises(KabuRateLimitError) as exc:
            check_response({"Code": 4002006, "Message": "rate limit"}, 200)
        assert exc.value.code == 4002006

    def test_other_nonzero_code_raises_generic_api_error(self):
        with pytest.raises(KabuApiError) as exc:
            check_response({"Code": 9999, "Message": "boom"}, 200)
        assert not isinstance(exc.value, KabuTokenExpiredError)
        assert not isinstance(exc.value, KabuRateLimitError)
        assert exc.value.code == 9999


class TestAuthHeaders:
    def test_returns_x_api_key(self):
        assert auth_headers("abc") == {"X-API-KEY": "abc"}

    def test_empty_token_raises(self):
        with pytest.raises(ValueError):
            auth_headers("")


class TestFetchToken:
    """Phase 8 §3.2 B1 / kabu skill R1+R3+R7+R10.

    POST {base}/token with {"APIPassword": ...} body. Returns the Token string
    on 2xx + Code 0. The function itself does NOT persist the token.
    """

    def setup_method(self):
        # Deferred import: fetch_token is not implemented yet (Phase 8 §3.2 B1).
        # Importing at class setup keeps collection GREEN for the other 3 test
        # classes while letting these 7 tests RED with ImportError until impl lands.
        from engine.exchanges.kabusapi_auth import fetch_token  # noqa: F401
        self._fetch_token = fetch_token

    async def test_returns_token_on_200_ok(self, httpx_mock: HTTPXMock):
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 0, "Token": "abcd1234efgh5678"},
        )
        token = await self._fetch_token("api-pw", env="verify")
        assert token == "abcd1234efgh5678"

    async def test_posts_api_password_in_json_body(self, httpx_mock: HTTPXMock):
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 0, "Token": "tkn"},
        )
        await self._fetch_token("my-secret-pw", env="verify")
        request = httpx_mock.get_request()
        assert request is not None
        import json as _json
        body = _json.loads(request.content)
        assert body == {"APIPassword": "my-secret-pw"}

    async def test_http_4xx_raises_api_error(self, httpx_mock: HTTPXMock):
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            status_code=401,
            json={"Code": 4001001, "Message": "Unauthorized"},
        )
        with pytest.raises(KabuApiError) as exc:
            await self._fetch_token("bad-pw", env="verify")
        assert exc.value.code == 4001001

    async def test_non_json_response_raises_connection_error(
        self, httpx_mock: HTTPXMock
    ):
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            content=b"<html>500 oops</html>",
            headers={"content-type": "text/html"},
        )
        with pytest.raises(KabuConnectionError):
            await self._fetch_token("pw", env="verify")

    async def test_token_key_missing_raises_connection_error(
        self, httpx_mock: HTTPXMock
    ):
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 0},
        )
        with pytest.raises(KabuConnectionError):
            await self._fetch_token("pw", env="verify")

    async def test_connect_error_raises_connection_error(
        self, httpx_mock: HTTPXMock
    ):
        def _boom(request):
            raise httpx.ConnectError("kabu body process not running")

        httpx_mock.add_callback(_boom, url=endpoint("token", env="verify"), method="POST")
        with pytest.raises(KabuConnectionError):
            await self._fetch_token("pw", env="verify")

    async def test_logs_only_masked_token_tail(
        self, httpx_mock: HTTPXMock, caplog
    ):
        """R10 — log line for the issued token must only show ***<last4>."""
        import logging

        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 0, "Token": "secrettoken9999"},
        )
        with caplog.at_level(logging.INFO):
            await self._fetch_token("pw", env="verify")
        joined = " ".join(rec.getMessage() for rec in caplog.records)
        assert "secrettoken9999" not in joined
        assert "***9999" in joined

    async def test_result_code_nonzero_raises_api_error(
        self, httpx_mock: HTTPXMock
    ):
        """OpenAPI TokenSuccess: ResultCode != 0 はエラーコード (kabu_STATION_API.yaml L5088)。
        現状は ResultCode を見ず Token 欠落で KabuConnectionError に倒れる → 認証失敗が
        接続障害として誤分類される。"""
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 4001003, "Message": "invalid APIPassword"},
        )
        with pytest.raises(KabuApiError) as exc:
            await self._fetch_token("bad-pw", env="verify")
        assert exc.value.code == 4001003
        assert not isinstance(exc.value, KabuConnectionError)

    async def test_result_code_4001005_raises_token_expired(
        self, httpx_mock: HTTPXMock
    ):
        """check_response の Code 4001005 マッピングと同じ classification を
        ResultCode 経路でも適用する。"""
        httpx_mock.add_response(
            url=endpoint("token", env="verify"),
            method="POST",
            json={"ResultCode": 4001005, "Message": "token expired"},
        )
        with pytest.raises(KabuTokenExpiredError) as exc:
            await self._fetch_token("pw", env="verify")
        assert exc.value.code == 4001005
