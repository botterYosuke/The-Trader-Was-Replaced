"""engine.strategy_runtime.run_buffer — RunBuffer writer for strategy backtest runs."""
from __future__ import annotations

import atexit
import json
import logging
import os
import signal
import subprocess
import sys
import tempfile
import time
import weakref
from datetime import datetime, timezone
from pathlib import Path
from typing import IO, Optional

from engine.strategy_runtime import strategy_loader

log = logging.getLogger(__name__)

_PREVIOUS_SIGTERM_HANDLER = None
_SIGTERM_HANDLER_INSTALLED = False
_ACTIVE_BUFFERS: "weakref.WeakSet[RunBuffer]" = weakref.WeakSet()


def get_run_buffer_base_dir() -> Path:
    if sys.platform == "win32":
        appdata = os.getenv("APPDATA")
        if appdata:
            return Path(appdata) / "flowsurface" / "run-buffer"
        return Path.home() / "AppData" / "Roaming" / "flowsurface" / "run-buffer"
    elif sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "flowsurface" / "run-buffer"
    else:
        return Path.home() / ".local" / "share" / "flowsurface" / "run-buffer"


def make_run_id(strategy_file: str, instrument: str) -> str:
    utc_sec = int(datetime.now(tz=timezone.utc).timestamp())
    stem = Path(strategy_file).stem
    instrument_clean = instrument.replace(".", "_")
    return f"{utc_sec}-{stem}-{instrument_clean}"


def _get_git_rev(cwd: Optional[Path] = None) -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=5,
            cwd=str(cwd) if cwd is not None else None,
        )
        if result.returncode == 0:
            rev = result.stdout.strip()
            return rev if rev else "unknown"
    except Exception:
        pass
    return "unknown"


def _sha256_file(path: str) -> str:
    # 算出本体は strategy_loader に集約。run メタは best-effort なので失敗は "unknown"。
    try:
        return strategy_loader.sha256_file(path)
    except Exception:
        return "unknown"


def _write_meta_atomic(meta_path: Path, meta: dict) -> None:
    parent = meta_path.parent
    fd, tmp_path_str = tempfile.mkstemp(dir=parent, suffix=".tmp")
    try:
        encoded = json.dumps(meta, ensure_ascii=False, indent=2).encode("utf-8")
        os.write(fd, encoded)
        os.fsync(fd)
    finally:
        try:
            os.close(fd)
        except OSError:
            pass

    last_exc: Optional[BaseException] = None
    for attempt in range(3):
        try:
            os.replace(tmp_path_str, str(meta_path))
            return
        except PermissionError as exc:
            last_exc = exc
            if attempt < 2:
                time.sleep(0.05)
                continue
        except Exception:
            try:
                os.unlink(tmp_path_str)
            except OSError:
                pass
            raise
    try:
        os.unlink(tmp_path_str)
    except OSError:
        pass
    assert last_exc is not None
    raise last_exc


def _install_sigterm_handler_once() -> None:
    global _PREVIOUS_SIGTERM_HANDLER, _SIGTERM_HANDLER_INSTALLED
    if _SIGTERM_HANDLER_INSTALLED:
        return
    if sys.platform == "win32":
        _SIGTERM_HANDLER_INSTALLED = True
        return
    try:
        _PREVIOUS_SIGTERM_HANDLER = signal.getsignal(signal.SIGTERM)
        signal.signal(signal.SIGTERM, _sigterm_handler_global)
        _SIGTERM_HANDLER_INSTALLED = True
    except (ValueError, OSError) as exc:
        log.debug("RunBuffer: SIGTERM handler not installed: %s", exc)


def _sigterm_handler_global(signum, frame) -> None:
    for rb in list(_ACTIVE_BUFFERS):
        try:
            rb.abort()
        except Exception as exc:
            log.warning("RunBuffer: SIGTERM abort failed: %s", exc)
    prev = _PREVIOUS_SIGTERM_HANDLER
    if callable(prev):
        try:
            prev(signum, frame)
            return
        except Exception:
            pass
    if prev == signal.SIG_IGN:
        return
    sys.exit(143)


class RunBuffer:
    """Writes fill / equity events to JSONL files under a run directory."""

    def __init__(
        self,
        *,
        run_id: str,
        strategy_file: str,
        scenario: Optional[dict],
        base_dir: Optional[Path] = None,
    ) -> None:
        self._run_id = run_id
        self._strategy_file = strategy_file
        self._scenario = scenario
        self._base_dir = base_dir if base_dir is not None else get_run_buffer_base_dir()
        self._run_dir = self._base_dir / run_id

        self._fills_fh: Optional[IO[str]] = None
        self._equity_fh: Optional[IO[str]] = None

        self._finished = False
        self._aborted = False

        self._run_dir.mkdir(parents=True, exist_ok=True)

        started_at = datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        meta = {
            "schema_version": 1,
            "run_id": run_id,
            "strategy_file": strategy_file,
            "strategy_sha256": _sha256_file(strategy_file),
            "git_rev": _get_git_rev(
                Path(strategy_file).resolve().parent if strategy_file else None
            ),
            "scenario": scenario,
            "started_at": started_at,
            "finished_at": None,
            "status": "running",
        }
        _write_meta_atomic(self._run_dir / "meta.json", meta)
        log.info("RunBuffer: started run_id=%s", run_id)

        _ACTIVE_BUFFERS.add(self)
        atexit.register(self._atexit_handler)
        _install_sigterm_handler_once()

    @property
    def run_id(self) -> str:
        return self._run_id

    @property
    def run_dir(self) -> Path:
        return self._run_dir

    def __enter__(self) -> "RunBuffer":
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        if not self._finished and not self._aborted:
            self.abort()
        self.close()

    def write_fill(self, event: dict) -> None:
        fh = self._get_fills_fh()
        fh.write(json.dumps(event, ensure_ascii=False) + "\n")
        fh.flush()

    def write_equity(self, event: dict) -> None:
        fh = self._get_equity_fh()
        fh.write(json.dumps(event, ensure_ascii=False) + "\n")
        fh.flush()

    def _get_fills_fh(self) -> IO[str]:
        if self._fills_fh is None:
            self._fills_fh = open(self._run_dir / "fills.jsonl", "a", encoding="utf-8")
        return self._fills_fh

    def _get_equity_fh(self) -> IO[str]:
        if self._equity_fh is None:
            self._equity_fh = open(self._run_dir / "equity.jsonl", "a", encoding="utf-8")
        return self._equity_fh

    def finish(self) -> None:
        if self._finished or self._aborted:
            return
        try:
            self._flush_and_fsync_all_jsonl()
        except OSError as exc:
            log.warning(
                "RunBuffer: fsync failed for run_id=%s, falling through to abort: %s",
                self._run_id,
                exc,
            )
            self.abort()
            raise
        finished_at = datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        self._update_meta_status("finished", finished_at=finished_at)
        self._finished = True
        self._deregister_atexit()
        self.close()
        log.info("RunBuffer: finished run_id=%s", self._run_id)

    def abort(self) -> None:
        if self._finished or self._aborted:
            return
        try:
            self._update_meta_status("aborted")
        except Exception as exc:
            log.warning(
                "RunBuffer: atomic abort write failed for run_id=%s: %s; falling back",
                self._run_id,
                exc,
            )
            self._best_effort_write_aborted()
        self._aborted = True
        self._deregister_atexit()
        self.close()
        log.info("RunBuffer: aborted run_id=%s", self._run_id)

    def _best_effort_write_aborted(self) -> None:
        meta_path = self._run_dir / "meta.json"
        try:
            try:
                meta = json.loads(meta_path.read_text(encoding="utf-8"))
            except (FileNotFoundError, json.JSONDecodeError):
                meta = {
                    "schema_version": 1,
                    "run_id": self._run_id,
                    "strategy_file": self._strategy_file,
                    "status": "running",
                }
            meta["status"] = "aborted"
            meta_path.write_text(
                json.dumps(meta, ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
        except Exception as exc:
            log.warning(
                "RunBuffer: best-effort abort write failed for run_id=%s: %s",
                self._run_id,
                exc,
            )

    def _atexit_handler(self) -> None:
        if self._finished or self._aborted:
            return
        try:
            self.abort()
        except Exception as exc:
            log.warning(
                "RunBuffer: atexit handler failed for run_id=%s: %s",
                self._run_id,
                exc,
            )

    def _deregister_atexit(self) -> None:
        try:
            atexit.unregister(self._atexit_handler)
        except Exception:
            pass
        try:
            _ACTIVE_BUFFERS.discard(self)
        except Exception:
            pass

    def _flush_and_fsync_all_jsonl(self) -> None:
        for fh in (self._fills_fh, self._equity_fh):
            if fh is not None:
                fh.flush()
                os.fsync(fh.fileno())

    def _update_meta_status(self, status: str, *, finished_at: Optional[str] = None) -> None:
        meta_path = self._run_dir / "meta.json"
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            log.warning("RunBuffer: meta.json read failed, creating fresh meta")
            meta = {
                "schema_version": 1,
                "run_id": self._run_id,
                "strategy_file": self._strategy_file,
                "strategy_sha256": "unknown",
                "git_rev": "unknown",
                "scenario": self._scenario,
                "started_at": datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "finished_at": None,
                "status": "running",
            }
        meta["status"] = status
        if finished_at is not None:
            meta["finished_at"] = finished_at
        _write_meta_atomic(meta_path, meta)

    def close(self) -> None:
        for attr in ("_fills_fh", "_equity_fh"):
            fh = getattr(self, attr)
            if fh is not None:
                try:
                    fh.close()
                except OSError:
                    pass
                setattr(self, attr, None)
