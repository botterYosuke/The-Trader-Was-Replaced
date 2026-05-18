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
