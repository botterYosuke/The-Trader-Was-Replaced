"""Tests for Phase 8 §3.2.1 login_dialog_runner skeleton."""

import json

import pytest

from engine.live import login_dialog_runner
from engine.live.login_dialog_runner import main, parse_args


def _parse_stdout(captured) -> dict:
    """captured.out を NDJSON 1 行とみなして dict 化。"""
    out = captured.out.strip()
    assert out.count("\n") == 0, f"expected single line, got: {out!r}"
    return json.loads(out)


def test_parse_args_ok():
    ns = parse_args(["--venue", "tachibana", "--env", "demo"])
    assert ns.venue == "tachibana"
    assert ns.env == "demo"


def test_unknown_venue(capsys, monkeypatch):
    monkeypatch.setattr(login_dialog_runner, "try_create_tk", lambda: True)
    rc = main(["--venue", "unknown", "--env", "demo"])
    captured = capsys.readouterr()
    payload = _parse_stdout(captured)
    assert rc == 0
    assert payload == {"type": "result", "success": False, "error_code": "UNKNOWN_VENUE"}


def test_missing_venue(capsys, monkeypatch):
    monkeypatch.setattr(login_dialog_runner, "try_create_tk", lambda: True)
    rc = main(["--env", "demo"])
    captured = capsys.readouterr()
    payload = _parse_stdout(captured)
    assert rc == 0
    assert payload["error_code"] == "UNKNOWN_VENUE"


def test_invalid_env(capsys, monkeypatch):
    monkeypatch.setattr(login_dialog_runner, "try_create_tk", lambda: True)
    rc = main(["--venue", "tachibana", "--env", "staging"])
    captured = capsys.readouterr()
    payload = _parse_stdout(captured)
    assert rc == 0
    assert payload["error_code"] == "INVALID_ENV"


def test_headless_no_display(capsys, monkeypatch):
    monkeypatch.setattr(login_dialog_runner, "try_create_tk", lambda: False)
    rc = main(["--venue", "tachibana", "--env", "demo"])
    captured = capsys.readouterr()
    payload = _parse_stdout(captured)
    assert rc == 0
    assert payload["error_code"] == "NO_DISPLAY_AVAILABLE"


def test_tk_available_not_implemented(capsys, monkeypatch):
    monkeypatch.setattr(login_dialog_runner, "try_create_tk", lambda: True)
    rc = main(["--venue", "tachibana", "--env", "demo"])
    captured = capsys.readouterr()
    payload = _parse_stdout(captured)
    assert rc == 0
    assert payload["error_code"] == "NOT_IMPLEMENTED"
    assert payload["type"] == "result"
    assert payload["success"] is False


def test_stdout_is_single_ndjson_line(capsys, monkeypatch):
    monkeypatch.setattr(login_dialog_runner, "try_create_tk", lambda: False)
    main(["--venue", "tachibana", "--env", "demo"])
    captured = capsys.readouterr()
    raw = captured.out
    assert raw.endswith("\n")
    assert raw.count("\n") == 1
    payload = json.loads(raw.strip())
    assert isinstance(payload, dict)
    assert payload.get("type") == "result"
