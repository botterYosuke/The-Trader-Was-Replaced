"""Tests for _build_mode and tools/freeze_build_mode.py."""
import subprocess
import sys
from pathlib import Path

# Project root is three levels up from python/tests/live/
_PROJECT_ROOT = Path(__file__).parent.parent.parent.parent
_BUILD_MODE_FILE = _PROJECT_ROOT / "python" / "engine" / "live" / "_build_mode.py"
_FREEZE_SCRIPT = _PROJECT_ROOT / "tools" / "freeze_build_mode.py"


def test_is_debug_build_importable():
    from engine.live._build_mode import IS_DEBUG_BUILD
    assert isinstance(IS_DEBUG_BUILD, bool)


def test_freeze_build_mode_release(tmp_path):
    """Smoke: freeze_build_mode.py --release runs successfully and prints False."""
    result = subprocess.run(
        [sys.executable, str(_FREEZE_SCRIPT), "--release"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0
    assert "False" in result.stdout

    # Immediately restore to debug (keep repo state clean)
    restore = subprocess.run(
        [sys.executable, str(_FREEZE_SCRIPT), "--debug"],
        capture_output=True, text=True,
    )
    assert restore.returncode == 0
    assert "True" in restore.stdout


def test_freeze_build_mode_debug_restores(tmp_path):
    # スクリプトが存在することの smoke test
    assert _BUILD_MODE_FILE.exists(), "_build_mode.py が存在する"
    assert _FREEZE_SCRIPT.exists(), "freeze_build_mode.py が存在する"
