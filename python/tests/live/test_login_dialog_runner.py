"""Tests for Phase 8 §3.2.1 login_dialog_runner."""

import json
from unittest.mock import patch

import pytest

from engine.live.login_dialog_runner import main, parse_args


def test_missing_cred_path_for_kabu():
    # --cred-path なし
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        with patch("engine.live.login_dialog_runner.try_create_tk", return_value=True):
            code = main(["--venue", "kabu", "--env", "verify"])
    assert code == 0
    assert result_lines[0]["error_code"] == "MISSING_CRED_PATH"


def test_invalid_env_tachibana():
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        code = main(["--venue", "tachibana", "--env", "verify"])
    assert code == 0
    assert result_lines[0]["error_code"] == "INVALID_ENV"


def test_invalid_env_kabu():
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        code = main(["--venue", "kabu", "--env", "demo"])
    assert code == 0
    assert result_lines[0]["error_code"] == "INVALID_ENV"


def test_no_display_available():
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        with patch("engine.live.login_dialog_runner.try_create_tk", return_value=False):
            code = main(["--venue", "tachibana", "--env", "demo"])
    assert code == 0
    assert result_lines[0]["error_code"] == "NO_DISPLAY_AVAILABLE"


def test_tachibana_success_dispatches_run_dialog():
    result_lines = []
    mock_result = {"success": True, "error_code": ""}
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        with patch("engine.live.login_dialog_runner.try_create_tk", return_value=True):
            with patch("engine.exchanges.tachibana_login_flow.run_dialog", return_value=mock_result):
                code = main(["--venue", "tachibana", "--env", "demo"])
    assert code == 0
    assert result_lines[0]["success"] is True


def test_unknown_venue():
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        code = main(["--venue", "unknown", "--env", "demo"])
    assert code == 0
    assert result_lines[0]["error_code"] == "UNKNOWN_VENUE"


def test_missing_required_arg_returns_1():
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        code = main(["--env", "demo"])
    assert code == 1
    assert result_lines[0]["error_code"] == "MISSING_REQUIRED_ARG"


def test_parse_args_ok():
    ns = parse_args(["--venue", "tachibana", "--env", "demo"])
    assert ns.venue == "tachibana"
    assert ns.env == "demo"


def test_parse_args_cred_path_default():
    ns = parse_args(["--venue", "kabu", "--env", "verify", "--cred-path", "/tmp/cred.json"])
    assert ns.cred_path == "/tmp/cred.json"


def test_kabu_prod_valid_env(monkeypatch):
    monkeypatch.setenv("KABU_ALLOW_PROD", "1")
    result_lines = []
    mock_result = {"success": True, "error_code": ""}
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        with patch("engine.live.login_dialog_runner.try_create_tk", return_value=True):
            with patch("engine.exchanges.kabusapi_login_flow.run_dialog", return_value=mock_result):
                code = main(["--venue", "kabu", "--env", "prod", "--cred-path", "/tmp/c.json"])
    assert code == 0
    assert result_lines[0]["success"] is True


def test_kabu_prod_not_allowed_without_env_flag(monkeypatch):
    """Fix #12: prod env_hint で KABU_ALLOW_PROD!=1 なら PROD_NOT_ALLOWED で前段拒否。"""
    monkeypatch.delenv("KABU_ALLOW_PROD", raising=False)
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        code = main(["--venue", "kabu", "--env", "prod", "--cred-path", "/tmp/c.json"])
    assert code == 0
    assert result_lines[0]["success"] is False
    assert result_lines[0]["error_code"] == "PROD_NOT_ALLOWED"


def test_tachibana_prod_not_allowed_without_env_flag(monkeypatch):
    monkeypatch.delenv("TACHIBANA_ALLOW_PROD", raising=False)
    result_lines = []
    with patch("engine.live.login_dialog_runner.emit", side_effect=result_lines.append):
        code = main(["--venue", "tachibana", "--env", "prod"])
    assert code == 0
    assert result_lines[0]["success"] is False
    assert result_lines[0]["error_code"] == "PROD_NOT_ALLOWED"
