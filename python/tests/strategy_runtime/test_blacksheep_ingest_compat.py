"""Step 5 compatibility test: ingest_run.py consumes a strategy_replay run-buffer.

Marked `slow` — skipped in normal CI.
Skipped entirely when the blacksheep repo is not present on disk.

What this test proves:
    1. `engine.strategy_replay` CLI creates a valid run-buffer (meta/fills/equity)
    2. `ingest_run.py --e-station <this_repo>` resolves `engine.summary` via shim
    3. Bronze/Silver/wiki layers are written correctly

Run manually:
    uv run python -m pytest python/tests/strategy_runtime/test_blacksheep_ingest_compat.py -v -m slow
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

_BLACKSHEEP_ROOT = Path("C:/Users/sasai/Documents/🐃_blacksheep")
_INGEST_RUN_PY = _BLACKSHEEP_ROOT / "scripts" / "ingest_run.py"
_FIXTURE_DIR = Path(__file__).parent.parent / "fixtures" / "strategies"
_FAKE_STRATEGY_PATH = _FIXTURE_DIR / "fake_market_buy_once.py"
# The-Trader-Was-Replaced repo root (4 levels up from this file)
_THIS_REPO = Path(__file__).resolve().parents[4]

# ---------------------------------------------------------------------------
# Skip guard
# ---------------------------------------------------------------------------

pytestmark = [
    pytest.mark.slow,
    pytest.mark.skipif(
        not _INGEST_RUN_PY.exists(),
        reason=f"blacksheep not found at {_BLACKSHEEP_ROOT}",
    ),
]


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


def _make_bars_json(tmp_path: Path) -> Path:
    bars = [
        {
            "ts_event": i * 86_400_000_000_000,
            "ts_init": i * 86_400_000_000_000,
            "open": str(1000.0 + i * 10),
            "high": str(1010.0 + i * 10),
            "low": str(990.0 + i * 10),
            "close": str(1005.0 + i * 10),
            "volume": "1000",
            "granularity": "Daily",
        }
        for i in range(5)
    ]
    p = tmp_path / "bars.json"
    p.write_text(json.dumps({"1301.TSE": bars}), encoding="utf-8")
    return p


@pytest.fixture()
def run_buffer(tmp_path: Path):
    """Create a run-buffer via CLI and return (rb_dir, run_id)."""
    bars_json = _make_bars_json(tmp_path)
    rb_dir = tmp_path / "rb"

    result = subprocess.run(
        [
            sys.executable, "-m", "engine.strategy_replay", "run",
            "--strategy", str(_FAKE_STRATEGY_PATH),
            "--bars-json", str(bars_json),
            "--run-buffer-dir", str(rb_dir),
        ],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, f"CLI failed: {result.stderr}"

    run_dirs = [d for d in rb_dir.iterdir() if d.is_dir()]
    assert len(run_dirs) == 1
    return rb_dir, run_dirs[0].name


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _run_ingest(run_id: str, rb_dir: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [
            sys.executable,
            str(_INGEST_RUN_PY),
            run_id,
            "--source", str(rb_dir),
            "--e-station", str(_THIS_REPO),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        cwd=str(_BLACKSHEEP_ROOT),
    )


def _wiki_page_path(run_id: str) -> Path:
    parts = run_id.split("-")
    strategy = "-".join(parts[1:-1])
    timestamp = parts[0]
    return _BLACKSHEEP_ROOT / "wiki" / "runs" / f"{strategy}-{timestamp}.md"


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class TestIngestReturnCode:
    def test_ingest_exits_zero(self, run_buffer):
        rb_dir, run_id = run_buffer
        result = _run_ingest(run_id, rb_dir)
        assert result.returncode == 0, (
            f"ingest_run.py exited non-zero\nstdout: {result.stdout}\nstderr: {result.stderr}"
        )

    def test_ingest_no_error_in_stderr(self, run_buffer):
        rb_dir, run_id = run_buffer
        result = _run_ingest(run_id, rb_dir)
        # "WARNING" は許容するが "ERROR" や traceback は失敗
        stderr = result.stderr
        assert "ERROR" not in stderr, f"ERROR in stderr:\n{stderr}"
        assert "Traceback" not in stderr, f"Traceback in stderr:\n{stderr}"


class TestBronzeLayer:
    def test_bronze_meta_json_exists(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        assert (_BLACKSHEEP_ROOT / "raw" / "replay-runs" / run_id / "meta.json").exists()

    def test_bronze_fills_jsonl_exists(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        assert (_BLACKSHEEP_ROOT / "raw" / "replay-runs" / run_id / "fills.jsonl").exists()

    def test_bronze_equity_jsonl_exists(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        assert (_BLACKSHEEP_ROOT / "raw" / "replay-runs" / run_id / "equity.jsonl").exists()

    def test_bronze_meta_schema_version(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        meta = json.loads(
            (_BLACKSHEEP_ROOT / "raw" / "replay-runs" / run_id / "meta.json").read_text()
        )
        assert meta["schema_version"] == 1

    def test_bronze_meta_status_finished(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        meta = json.loads(
            (_BLACKSHEEP_ROOT / "raw" / "replay-runs" / run_id / "meta.json").read_text()
        )
        assert meta["status"] == "finished"


class TestSilverLayer:
    def test_silver_summary_json_exists(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        assert (_BLACKSHEEP_ROOT / "Silver" / "runs" / run_id / "summary.json").exists()

    def test_silver_summary_has_required_keys(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        summary = json.loads(
            (_BLACKSHEEP_ROOT / "Silver" / "runs" / run_id / "summary.json").read_text()
        )
        for key in ("total_pnl", "max_drawdown", "trade_count",
                    "win_rate", "fee_total", "equity_points", "fills_count"):
            assert key in summary, f"summary.json missing key: {key}"

    def test_silver_equity_points_gt_zero(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        summary = json.loads(
            (_BLACKSHEEP_ROOT / "Silver" / "runs" / run_id / "summary.json").read_text()
        )
        assert summary["equity_points"] > 0

    def test_silver_fills_count_gte_1(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        summary = json.loads(
            (_BLACKSHEEP_ROOT / "Silver" / "runs" / run_id / "summary.json").read_text()
        )
        assert summary["fills_count"] >= 1


class TestWikiLayer:
    def test_wiki_page_exists(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        assert _wiki_page_path(run_id).exists()

    def test_wiki_page_has_auto_metrics_block(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        text = _wiki_page_path(run_id).read_text(encoding="utf-8")
        assert "<!-- AUTO:METRICS:START -->" in text
        assert "<!-- AUTO:METRICS:END -->" in text

    def test_wiki_page_has_auto_links_block(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        text = _wiki_page_path(run_id).read_text(encoding="utf-8")
        assert "<!-- AUTO:LINKS:START -->" in text
        assert "<!-- AUTO:LINKS:END -->" in text

    def test_wiki_page_embeds_run_id(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        text = _wiki_page_path(run_id).read_text(encoding="utf-8")
        assert run_id in text

    def test_wiki_re_ingest_is_idempotent(self, run_buffer):
        rb_dir, run_id = run_buffer
        _run_ingest(run_id, rb_dir)
        first_text = _wiki_page_path(run_id).read_text(encoding="utf-8")
        # second ingest — --force re-copies Bronze but wiki should not change
        _run_ingest(run_id, rb_dir)
        second_text = _wiki_page_path(run_id).read_text(encoding="utf-8")
        assert first_text == second_text, "wiki page changed on re-ingest (not idempotent)"


class TestEngineSummaryShim:
    """Verify the shim works when imported the same way ingest_run.py does it."""

    def test_shim_importable_via_sys_path_insert(self, tmp_path):
        """Simulate _ingest_silver's sys.path manipulation."""
        import importlib
        import sys as _sys

        python_dir = str((_THIS_REPO / "python").resolve())
        if python_dir not in _sys.path:
            _sys.path.insert(0, python_dir)

        # Force re-import in case it's cached
        for mod in ("engine.summary", "engine.strategy_runtime.summary"):
            _sys.modules.pop(mod, None)

        from engine.summary import compute_summary, write_summary_json  # noqa: F401
        assert callable(compute_summary)
        assert callable(write_summary_json)

    def test_shim_compute_summary_on_empty_dir(self, tmp_path):
        import sys as _sys
        python_dir = str((_THIS_REPO / "python").resolve())
        if python_dir not in _sys.path:
            _sys.path.insert(0, python_dir)

        for mod in ("engine.summary", "engine.strategy_runtime.summary"):
            _sys.modules.pop(mod, None)

        from engine.summary import compute_summary
        result = compute_summary(tmp_path)
        assert result["equity_points"] == 0
        assert result["fills_count"] == 0
