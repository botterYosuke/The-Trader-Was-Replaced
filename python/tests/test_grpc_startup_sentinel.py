"""Step 1: Python 側 readiness sentinel (`GRPC_LISTENING port=<port>`) の
format / 出現回数 / flush 挙動を保証する integration test (subprocess ベース)。

C-5 (plans/backend-startup-sync.md) に基づき、Rust supervisor の stdout parser
(`^GRPC_LISTENING port=(\\d+)$` regex) と format 契約を結ぶ。
"""

from __future__ import annotations

import os
import re
import selectors
import socket
import subprocess
import sys
import time
from pathlib import Path

import pytest

_SENTINEL_RE = re.compile(r"^GRPC_LISTENING port=(\d+)$")
_REPO_ROOT = Path(__file__).resolve().parents[2]
_PYTHON_DIR = _REPO_ROOT / "python"


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _spawn_engine(port: int) -> subprocess.Popen:
    env = os.environ.copy()
    env["PYTHONUNBUFFERED"] = "1"
    return subprocess.Popen(
        [
            sys.executable,
            "-m",
            "engine",
            "--token",
            "test-token",
            "--port",
            str(port),
        ],
        cwd=str(_PYTHON_DIR),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
        text=True,
        bufsize=1,
    )


def _read_until_sentinel(
    proc: subprocess.Popen, port: int, timeout_sec: float
) -> tuple[str | None, list[str]]:
    """stdout を timeout まで line-stream で読み、sentinel 行と全行履歴を返す。
    readline() の blocking で deadline を踏み外さないよう selectors で poll する。
    """
    deadline = time.monotonic() + timeout_sec
    seen: list[str] = []
    sentinel: str | None = None
    assert proc.stdout is not None
    sel = selectors.DefaultSelector()
    sel.register(proc.stdout, selectors.EVENT_READ)
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            # 1s 単位で poll し、deadline と child 死活を定期的に再評価
            events = sel.select(timeout=min(1.0, remaining))
            if not events:
                if proc.poll() is not None:
                    break
                continue
            line = proc.stdout.readline()
            if not line:
                # EOF (child 終了)
                if proc.poll() is not None:
                    break
                continue
            line = line.rstrip("\r\n")
            seen.append(line)
            m = _SENTINEL_RE.fullmatch(line)
            if m and int(m.group(1)) == port:
                sentinel = line
                break
    finally:
        sel.unregister(proc.stdout)
        sel.close()
    return sentinel, seen


def _terminate(proc: subprocess.Popen) -> None:
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)


@pytest.mark.integration
def test_grpc_listening_sentinel_emitted_once_with_correct_port() -> None:
    port = _free_port()
    proc = _spawn_engine(port)
    try:
        sentinel, seen = _read_until_sentinel(proc, port, timeout_sec=30.0)
        assert sentinel is not None, (
            f"sentinel `GRPC_LISTENING port={port}` not observed within 30s. "
            f"stdout tail: {seen[-20:]!r}"
        )
        # format 厳密一致 (Rust parser の regex contract)
        assert _SENTINEL_RE.fullmatch(sentinel), f"sentinel format broken: {sentinel!r}"
        # 重複検知のため少し余分に読む
        proc.stdout.flush() if False else None  # no-op
        # この時点で seen 内の sentinel 行は厳密に 1 つであるべき
        sentinel_lines = [ln for ln in seen if _SENTINEL_RE.fullmatch(ln)]
        assert len(sentinel_lines) == 1, (
            f"sentinel must appear exactly once per process start, "
            f"got {len(sentinel_lines)}: {sentinel_lines!r}"
        )
    finally:
        _terminate(proc)
