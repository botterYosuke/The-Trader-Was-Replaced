"""Step 3 (backend-startup-sync): Shutdown rpc の subprocess 終端契約を検証する
integration test。

C-6 (plans/backend-startup-sync.md):
- (a) grace_seconds=0 → 即時 shutdown、subprocess は exit code 0 で 3s 以内に終了
- (b) grace_seconds>0 → accepted=True を返し、grace+2s 以内に exit code 0
- (c) 二重 Shutdown → 1 回目 accepted=True、2 回目 accepted=False
      error_code="ALREADY_SHUTTING_DOWN"
"""

from __future__ import annotations

import os
import queue
import re
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

import grpc
import pytest

from engine.proto import engine_pb2, engine_pb2_grpc

_SENTINEL_RE = re.compile(r"^GRPC_LISTENING port=(\d+)$")
_REPO_ROOT = Path(__file__).resolve().parents[2]
_PYTHON_DIR = _REPO_ROOT / "python"
_TOKEN = "test-token"


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
            _TOKEN,
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


def _drain_stdout(proc: subprocess.Popen, line_q: "queue.Queue[str | None]") -> None:
    """Background reader: push each stdout line onto the queue, then a None EOF marker.
    A thread is used instead of selectors/select because select() does not work on
    pipe handles on Windows (only sockets), which would raise WinError 10038."""
    try:
        assert proc.stdout is not None
        for line in proc.stdout:
            line_q.put(line.rstrip("\r\n"))
    finally:
        line_q.put(None)


def _wait_for_sentinel(proc: subprocess.Popen, port: int, timeout_sec: float = 30.0) -> None:
    """sentinel を観測するまで stdout を line-stream する。観測できなければ fail。"""
    deadline = time.monotonic() + timeout_sec
    seen: list[str] = []
    assert proc.stdout is not None
    line_q: "queue.Queue[str | None]" = queue.Queue()
    reader = threading.Thread(target=_drain_stdout, args=(proc, line_q), daemon=True)
    reader.start()
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError(
                f"sentinel for port={port} not observed within {timeout_sec}s. "
                f"stdout tail: {seen[-20:]!r}"
            )
        try:
            line = line_q.get(timeout=min(1.0, remaining))
        except queue.Empty:
            if proc.poll() is not None:
                raise AssertionError(
                    f"subprocess exited before sentinel (rc={proc.returncode}). "
                    f"stdout: {seen!r}"
                )
            continue
        if line is None:
            if proc.poll() is not None:
                raise AssertionError(
                    f"EOF before sentinel (rc={proc.returncode}). stdout: {seen!r}"
                )
            continue
        seen.append(line)
        m = _SENTINEL_RE.fullmatch(line)
        if m and int(m.group(1)) == port:
            return


def _terminate(proc: subprocess.Popen) -> None:
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)


def _make_stub(port: int) -> tuple[grpc.Channel, engine_pb2_grpc.DataEngineStub]:
    channel = grpc.insecure_channel(f"127.0.0.1:{port}")
    stub = engine_pb2_grpc.DataEngineStub(channel)
    return channel, stub


@pytest.mark.integration
def test_shutdown_grace_zero_exits_immediately() -> None:
    """(a) grace_seconds=0 → accepted=True、subprocess は 3s 以内に exit 0。"""
    port = _free_port()
    proc = _spawn_engine(port)
    try:
        _wait_for_sentinel(proc, port)
        channel, stub = _make_stub(port)
        try:
            resp = stub.Shutdown(
                engine_pb2.ShutdownRequest(token=_TOKEN, grace_seconds=0),
                timeout=5.0,
            )
            assert resp.accepted is True, f"expected accepted=True, got {resp!r}"
            assert resp.error_code == "", f"expected empty error_code, got {resp.error_code!r}"
        finally:
            channel.close()
        rc = proc.wait(timeout=3.0)
        assert rc == 0, f"expected exit code 0, got {rc}"
    finally:
        _terminate(proc)


@pytest.mark.integration
def test_shutdown_with_grace_exits_within_window() -> None:
    """(b) grace_seconds=3 → accepted=True、subprocess は grace+2s 以内に exit 0。"""
    port = _free_port()
    proc = _spawn_engine(port)
    grace = 3
    try:
        _wait_for_sentinel(proc, port)
        channel, stub = _make_stub(port)
        try:
            resp = stub.Shutdown(
                engine_pb2.ShutdownRequest(token=_TOKEN, grace_seconds=grace),
                timeout=5.0,
            )
            assert resp.accepted is True, f"expected accepted=True, got {resp!r}"
            assert resp.error_code == ""
        finally:
            channel.close()
        rc = proc.wait(timeout=grace + 2.0)
        assert rc == 0, f"expected exit code 0, got {rc}"
    finally:
        _terminate(proc)


@pytest.mark.integration
def test_shutdown_second_call_rejected_as_already_shutting_down() -> None:
    """(c) 2 回目の Shutdown は accepted=False, error_code='ALREADY_SHUTTING_DOWN'。
    1 回目は grace_seconds=2 で実 exit まで余裕を取り、2 回目を間に合わせる。
    """
    port = _free_port()
    proc = _spawn_engine(port)
    try:
        _wait_for_sentinel(proc, port)
        channel, stub = _make_stub(port)
        try:
            first = stub.Shutdown(
                engine_pb2.ShutdownRequest(token=_TOKEN, grace_seconds=2),
                timeout=5.0,
            )
            assert first.accepted is True, f"first call: {first!r}"
            assert first.error_code == ""

            second = stub.Shutdown(
                engine_pb2.ShutdownRequest(token=_TOKEN, grace_seconds=2),
                timeout=5.0,
            )
            assert second.accepted is False, f"second call: {second!r}"
            assert second.error_code == "ALREADY_SHUTTING_DOWN", (
                f"expected ALREADY_SHUTTING_DOWN, got {second.error_code!r}"
            )
        finally:
            channel.close()
        rc = proc.wait(timeout=4.0)
        assert rc == 0, f"expected exit code 0, got {rc}"
    finally:
        _terminate(proc)
