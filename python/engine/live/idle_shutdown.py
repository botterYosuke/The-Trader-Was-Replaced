"""Idle shutdown monitor — Phase 9 Step 8 / §3.7。

独立起動 (Bevy supervisor 配下でない) backend が、どの RPC も 60 秒来なければ自己 shutdown
する。CLI から手動で `python -m engine` を起動して放置したプロセスが居残らないようにする運用用。

責務:
- `LastRequestClock` を gRPC `ServerInterceptor` が全 RPC で `touch()` し「最後に要求が来た時刻」を
  記録する。
- `IdleShutdownMonitor` (daemon thread) が `check_interval_s` 毎に idle 時間を確認し、
  `idle_timeout_s` を超えたら `on_idle()`（= `process_lifecycle.start_shutdown(grace=2)`）を 1 回
  呼んで終了する。start_shutdown が teardown（kabu なら `unregister/all`）→ `server.stop` を担う。

設計判断:
- **threading ベース (asyncio ではない)**: backend は `ThreadPoolExecutor` ベースの
  **同期処理**で動作し、interceptor も monitor も worker/daemon thread 上で走る。計画書 §3.7 の
  「asyncio.Lock / background asyncio task」前提は**ドリフト訂正**（SecretVault が threading.Lock に
  したのと同じ理由 — 単一 asyncio loop は存在しない）。`LastRequestClock` は `threading.Lock` で保護。
- **supervisor 配下では無効**: Bevy が spawn したときは `BACKEND_SUPERVISED=1` を立てる。その場合は
  monitor を起動しない（プロセス寿命は supervisor が握る。`should_enable_idle_shutdown` で判定）。
- `time_source` / `sleep` 注入でテストは実時間を消費せず決定論的に回す。
"""
from __future__ import annotations

import logging
import threading
import time as _time
from typing import Callable, Optional, Protocol

_LOG = logging.getLogger(__name__)

# §3.7 の既定値。Bevy supervisor 配下では monitor 自体を起動しない。
_DEFAULT_IDLE_TIMEOUT_S = 60.0
_DEFAULT_CHECK_INTERVAL_S = 5.0
_SHUTDOWN_GRACE_S = 2


class _IdleClock(Protocol):
    def idle_seconds(self) -> float: ...


def should_enable_idle_shutdown(environ: dict[str, str]) -> bool:
    """Bevy supervisor 配下 (`BACKEND_SUPERVISED=1`) では idle shutdown を無効化する。

    純粋関数 (env 辞書を受ける) なのでテストが os.environ を汚さずに判定できる。
    """
    return environ.get("BACKEND_SUPERVISED") != "1"


class LastRequestClock:
    """最後に RPC が来た時刻を保持する (monotonic)。同期サーバの worker thread 群から
    並行に `touch()` されるため `threading.Lock` で保護する。"""

    def __init__(self, *, time_source: Callable[[], float] = _time.monotonic) -> None:
        self._time = time_source
        self._lock = threading.Lock()
        self._last = time_source()

    def touch(self) -> None:
        with self._lock:
            self._last = self._time()

    def idle_seconds(self) -> float:
        with self._lock:
            return self._time() - self._last


class RequestActivityInterceptor:
    """全 RPC ディスパッチで `LastRequestClock.touch()` する server interceptor。

    `intercept_service` は RPC 到着ごとに 1 度呼ばれるため、これで「最後に要求が来た時刻」を
    記録できる (handler の実体には触れず continuation をそのまま返す)。"""

    def __init__(self, clock: LastRequestClock) -> None:
        self._clock = clock

    def intercept_service(self, continuation, handler_call_details):
        self._clock.touch()
        return continuation(handler_call_details)


class IdleShutdownMonitor:
    """idle 時間を `check_interval_s` 毎に監視し、超過したら `on_idle()` を 1 回呼ぶ daemon thread。"""

    def __init__(
        self,
        clock: _IdleClock,
        on_idle: Callable[[], None],
        *,
        idle_timeout_s: float = _DEFAULT_IDLE_TIMEOUT_S,
        check_interval_s: float = _DEFAULT_CHECK_INTERVAL_S,
    ) -> None:
        self._clock = clock
        self._on_idle = on_idle
        self._idle_timeout_s = idle_timeout_s
        self._check_interval_s = check_interval_s
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self._fired = False

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(
            target=self._run, daemon=True, name="idle_shutdown_monitor"
        )
        self._thread.start()

    def _run(self) -> None:
        # Event.wait(interval) を sleep として使う: stop されたら即 True を返して抜ける。
        while not self._stop.wait(self._check_interval_s):
            if self._clock.idle_seconds() > self._idle_timeout_s:
                self._fired = True
                _LOG.info(
                    "idle shutdown: no RPC for >%.0fs, initiating self-shutdown",
                    self._idle_timeout_s,
                )
                try:
                    self._on_idle()
                except Exception:  # noqa: BLE001 — best-effort; thread は終了する
                    _LOG.exception("idle shutdown on_idle() failed")
                return  # 1 回 fire したら監視終了 (shutdown 進行中)

    def stop(self) -> None:
        self._stop.set()
        thread = self._thread
        if thread is not None and thread is not threading.current_thread():
            thread.join(timeout=max(self._check_interval_s, 1.0) + 1.0)
        self._thread = None

    @property
    def fired(self) -> bool:
        return self._fired
