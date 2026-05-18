from __future__ import annotations

import json
import os
from datetime import date, datetime
from pathlib import Path
from zoneinfo import ZoneInfo


def session_file_path() -> Path:
    override = os.environ.get("TACHIBANA_SESSION_PATH")
    if override:
        return Path(override)
    base = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~/.cache")
    return Path(base) / "the-trader-was-replaced" / "tachibana" / "tachibana_session.json"


def save_session(session: dict) -> Path:
    path = session_file_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(session, ensure_ascii=False), encoding="utf-8")
    return path


def load_session() -> dict | None:
    path = session_file_path()
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


def is_session_valid_for_today(session: dict, *, today: date | None = None) -> bool:
    if today is None:
        today = datetime.now(ZoneInfo("Asia/Tokyo")).date()
    issued = session.get("issued_jst_date")
    if not issued:
        return False
    return str(issued) == today.isoformat()


def clear_session() -> None:
    path = session_file_path()
    if path.exists():
        path.unlink()
