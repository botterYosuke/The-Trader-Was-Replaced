import socket
import pytest
from engine.exchanges.kabusapi_login_form_state import (
    build_form_init, probe_station, validate_submission, EMPTY_FIELDS,
)


def test_build_form_init_allow_prod():
    fi = build_form_init("prod", env_dict={"KABU_ALLOW_PROD": "1"}, is_debug_build=False)
    assert fi.allow_prod is True
    assert fi.station_port == 18080


def test_build_form_init_release_no_dev_env():
    fi = build_form_init("verify", env_dict={"DEV_KABU_API_PASSWORD": "pw"}, is_debug_build=False)
    assert fi.dev_api_password is None


def test_build_form_init_debug_has_dev_env():
    fi = build_form_init("verify", env_dict={"DEV_KABU_API_PASSWORD": "pw"}, is_debug_build=True)
    assert fi.dev_api_password == "pw"


def test_build_form_init_verify_port():
    fi = build_form_init("verify", env_dict={}, is_debug_build=True)
    assert fi.station_port == 18081


def test_probe_station_refused(monkeypatch):
    def _fail(*args, **kwargs):
        raise ConnectionRefusedError("refused")
    monkeypatch.setattr(socket, "create_connection", _fail)
    assert probe_station() is False


def test_probe_station_success(monkeypatch):
    class _FakeConn:
        def __enter__(self): return self
        def __exit__(self, *a): pass
    monkeypatch.setattr(socket, "create_connection", lambda *a, **kw: _FakeConn())
    assert probe_station() is True


def test_validate_submission_empty():
    assert validate_submission("") == EMPTY_FIELDS


def test_validate_submission_valid():
    assert validate_submission("mypassword") is None


from engine.exchanges.kabusapi_login_form_state import (
    auth_failure_view,
    KABU_API_DISABLED,
    KABU_TOKEN_EXPIRED,
    KABU_STATION_NOT_RUNNING,
    AUTH_FAILED,
)


def test_auth_failure_view_api_disabled_blocks_retry():
    v = auth_failure_view(KABU_API_DISABLED)
    assert v.allow_retry is False
    assert "API 設定" in v.status_text


def test_auth_failure_view_token_expired_allows_retry():
    v = auth_failure_view(KABU_TOKEN_EXPIRED)
    assert v.allow_retry is True
    assert "トークン" in v.status_text


def test_auth_failure_view_station_not_running_blocks_retry():
    v = auth_failure_view(KABU_STATION_NOT_RUNNING)
    assert v.allow_retry is False
    assert KABU_STATION_NOT_RUNNING in v.status_text


def test_auth_failure_view_generic_auth_failed_allows_retry():
    v = auth_failure_view(AUTH_FAILED)
    assert v.allow_retry is True
    assert AUTH_FAILED in v.status_text
