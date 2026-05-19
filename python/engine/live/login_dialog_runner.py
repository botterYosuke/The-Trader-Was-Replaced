"""Phase 8 §3.2.1 ログインダイアログ subprocess の実装。

stdout: NDJSON 1 行 1 メッセージ
  {"type":"result","success":bool,"error_code":"..."}
stderr: 全 logging / warning / print

tkinter は遅延 import（headless 環境で module import 自体が落ちないように、
try_create_tk() の中だけで import する）。
"""

from __future__ import annotations

import argparse
import json
import logging
import sys

from engine.exchanges._env_guard import require_prod_env

VALID_VENUES = ("tachibana", "kabu")

_ENV_PER_VENUE: dict[str, set[str]] = {
    "tachibana": {"demo", "prod"},
    "kabu": {"verify", "prod"},
}

# logging は stderr へ
logging.basicConfig(stream=sys.stderr, level=logging.INFO)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="login_dialog_runner")
    parser.add_argument("--venue", required=True)
    parser.add_argument("--env", required=True)
    parser.add_argument("--cred-path", type=str, default="")
    return parser.parse_args(argv)


def emit(payload: dict) -> None:
    """stdout に 1 行 NDJSON を出して flush。"""
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def try_create_tk() -> bool:
    """tkinter import + Tk() 試行。例外なら False（headless 等）。"""
    try:
        import tkinter
        root = tkinter.Tk()
        root.withdraw()
        root.destroy()
        return True
    except Exception:
        return False


def _result(success: bool, error_code: str) -> dict:
    return {"type": "result", "success": success, "error_code": error_code}


def main(argv: list[str]) -> int:
    try:
        ns = parse_args(argv)
    except SystemExit:
        emit(_result(False, "MISSING_REQUIRED_ARG"))
        return 1

    if ns.venue not in VALID_VENUES:
        emit(_result(False, "UNKNOWN_VENUE"))
        return 0

    if ns.env not in _ENV_PER_VENUE.get(ns.venue, set()):
        emit(_result(False, "INVALID_ENV"))
        return 0

    if ns.env == "prod":
        allow_flag = "TACHIBANA_ALLOW_PROD" if ns.venue == "tachibana" else "KABU_ALLOW_PROD"
        try:
            require_prod_env(allow_flag)
        except RuntimeError:
            emit(_result(False, "PROD_NOT_ALLOWED"))
            return 0

    if ns.venue == "kabu" and not ns.cred_path:
        emit(_result(False, "MISSING_CRED_PATH"))
        return 0

    if not try_create_tk():
        emit(_result(False, "NO_DISPLAY_AVAILABLE"))
        return 0

    if ns.venue == "tachibana":
        from engine.exchanges.tachibana_login_flow import run_dialog
        result = run_dialog(env_hint=ns.env)
    else:  # kabu
        from engine.exchanges.kabusapi_login_flow import run_dialog
        result = run_dialog(env_hint=ns.env, cred_path=ns.cred_path)

    emit({"type": "result", **result})
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
