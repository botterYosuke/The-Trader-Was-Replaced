import json
from datetime import date
from pathlib import Path

import pytest


@pytest.fixture
def session_path(tmp_path, monkeypatch):
    p = tmp_path / "tachibana_session.json"
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(p))
    return p


def test_session_file_path_uses_env_override(session_path):
    from engine.exchanges.tachibana_file_store import session_file_path
    assert session_file_path() == session_path


def test_session_file_path_default_contains_tachibana(monkeypatch):
    monkeypatch.delenv("TACHIBANA_SESSION_PATH", raising=False)
    from engine.exchanges.tachibana_file_store import session_file_path
    p = session_file_path()
    assert "tachibana" in p.parts
    assert p.name == "tachibana_session.json"


def test_save_then_load_roundtrip(session_path):
    from engine.exchanges.tachibana_file_store import save_session, load_session
    data = {"sUrlRequest": "https://x", "issued_jst_date": "2026-05-18"}
    save_session(data)
    assert load_session() == data


def test_load_returns_none_when_missing(session_path):
    from engine.exchanges.tachibana_file_store import load_session
    assert load_session() is None


def test_load_returns_none_on_invalid_json(session_path):
    from engine.exchanges.tachibana_file_store import load_session
    session_path.parent.mkdir(parents=True, exist_ok=True)
    session_path.write_text("not json", encoding="utf-8")
    assert load_session() is None


def test_is_valid_today_matches(session_path):
    from engine.exchanges.tachibana_file_store import is_session_valid_for_today
    assert is_session_valid_for_today(
        {"issued_jst_date": "2026-05-18"}, today=date(2026, 5, 18)
    ) is True


def test_is_valid_today_mismatch(session_path):
    from engine.exchanges.tachibana_file_store import is_session_valid_for_today
    assert is_session_valid_for_today(
        {"issued_jst_date": "2026-05-17"}, today=date(2026, 5, 18)
    ) is False


def test_is_valid_today_missing_key(session_path):
    from engine.exchanges.tachibana_file_store import is_session_valid_for_today
    assert is_session_valid_for_today({}, today=date(2026, 5, 18)) is False


def test_clear_session_idempotent(session_path):
    from engine.exchanges.tachibana_file_store import clear_session, save_session
    clear_session()  # 無い状態で例外なし
    save_session({"x": 1})
    assert session_path.exists()
    clear_session()
    assert not session_path.exists()
    clear_session()  # 再 clear で例外なし
