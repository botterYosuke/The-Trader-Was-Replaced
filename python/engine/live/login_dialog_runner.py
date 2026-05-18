"""Phase 8 §3.2.1 ログインダイアログ subprocess の骨格。

stdout: NDJSON 1 行 1 メッセージ
  {"type":"result","success":bool,"error_code":"..."}
stderr: 全 logging / warning / print

tkinter は遅延 import（headless 環境で module import 自体が落ちないように、
try_create_tk() の中だけで import する）。

Phase 8 後半 HTTP/WS step で実際のログイン I/O を入れる。
"""

from __future__ import annotations

import argparse
import json
import logging
import sys

VALID_VENUES = ("tachibana", "kabu")
VALID_ENVS = ("demo", "prod", "verify")

# logging は stderr へ
logging.basicConfig(stream=sys.stderr, level=logging.INFO)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="login_dialog_runner")
    parser.add_argument("--venue", default=None)
    parser.add_argument("--env", default=None)
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
    ns = parse_args(argv)

    if ns.venue not in VALID_VENUES:
        emit(_result(False, "UNKNOWN_VENUE"))
        return 0

    if ns.env not in VALID_ENVS:
        emit(_result(False, "INVALID_ENV"))
        return 0

    if not try_create_tk():
        emit(_result(False, "NO_DISPLAY_AVAILABLE"))
        return 0

    # tkinter は使えるが本実装は Phase 8 後半
    emit(_result(False, "NOT_IMPLEMENTED"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
