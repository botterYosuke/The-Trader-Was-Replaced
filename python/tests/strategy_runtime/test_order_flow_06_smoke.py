"""Smoke test: order_flow_06 end-to-end with real catalog (slow).

Runs order_flow_06.py against the Jan 06-10 Minute catalog for a single
instrument (1301.TSE) using a minimal fixture universe JSON.

Marked @pytest.mark.slow — excluded from CI with ``-m 'not slow'``.
Skipped automatically if the real strategy file or catalog is not present.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import pytest

# ---------------------------------------------------------------------------
# Path constants
# ---------------------------------------------------------------------------

_REPO_ROOT = Path(__file__).parents[3]
_STRATEGY_PATH = Path(r"C:\Users\sasai\Documents\🐃_blacksheep\strategies\order_flow_06.py")
_CATALOG_PATH = _REPO_ROOT / "artifacts" / "jquants-catalog"
_FIXTURE_UNIVERSE = Path(__file__).parent / "fixtures" / "universe_1301_jan0610.json"

_STRATEGY_AVAILABLE = _STRATEGY_PATH.exists()
_CATALOG_AVAILABLE = _CATALOG_PATH.exists()

_SKIP_REASON = (
    "Real strategy file or catalog not available. "
    f"strategy={_STRATEGY_PATH.exists()} catalog={_CATALOG_PATH.exists()}"
)


# ---------------------------------------------------------------------------
# Smoke test
# ---------------------------------------------------------------------------


@pytest.mark.slow
@pytest.mark.skipif(
    not (_STRATEGY_AVAILABLE and _CATALOG_AVAILABLE),
    reason=_SKIP_REASON,
)
def test_order_flow_06_smoke_1instrument(tmp_path):
    """order_flow_06 runs on 1301.TSE (Jan 06-10) without exception.

    Acceptance criteria:
      - meta.status == "finished"
      - equity_points > 0   (strategy received bar events)
      - no exception raised
    """
    from engine.strategy_replay.cli import run_command
    from engine.strategy_runtime.strategy_loader import load

    # ── Load strategy + patch scenario to single instrument ──────────────────
    module, scenario, strategy_cls = load(_STRATEGY_PATH)

    # Override scenario to just 1301.TSE / Jan 06-10
    scenario = dict(
        scenario,
        instruments=["1301.TSE"],
        start="2025-01-06",
        end="2025-01-10",
        granularity="Daily",
        initial_cash=1_000_000,
    )
    # Remove instruments_ref so downstream doesn't re-resolve
    scenario.pop("instruments_ref", None)

    # ── Load bars from real catalog ───────────────────────────────────────────
    from engine.strategy_runtime.catalog_data_loader import load_bars_for_scenario
    bars_by_instrument = load_bars_for_scenario(str(_CATALOG_PATH), scenario)
    total_bars = sum(len(v) for v in bars_by_instrument.values())
    assert total_bars > 0, "no bars loaded from catalog"

    # ── Build warmup_loader ───────────────────────────────────────────────────
    from engine.strategy_runtime.warmup import make_catalog_warmup_loader
    warmup_loader = make_catalog_warmup_loader(_CATALOG_PATH)

    # ── RunBuffer ─────────────────────────────────────────────────────────────
    from engine.strategy_runtime.run_buffer import RunBuffer, make_run_id

    run_id = make_run_id(str(_STRATEGY_PATH), "1301.TSE")
    rb = RunBuffer(
        run_id=run_id,
        strategy_file=str(_STRATEGY_PATH),
        scenario=scenario,
        base_dir=tmp_path,
    )

    # ── engine_run ────────────────────────────────────────────────────────────
    from engine.strategy_runtime.engine_runner import run as engine_run
    from engine.strategy_runtime.summary import compute_summary, write_summary_json

    env_patch = {"STRATEGY_PARAM_UNIVERSE_JSON_PATH": str(_FIXTURE_UNIVERSE)}

    orig_env = {k: os.environ.get(k) for k in env_patch}
    try:
        os.environ.update(env_patch)
        engine_run(
            strategy_cls=strategy_cls,
            scenario=scenario,
            bars_by_instrument=bars_by_instrument,
            run_buffer=rb,
            strategy_init_kwargs={"warmup_loader": warmup_loader},
        )
    finally:
        for k, v in orig_env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v

    rb.finish()

    summary = compute_summary(rb.run_dir)
    write_summary_json(rb.run_dir, summary)

    meta = json.loads((rb.run_dir / "meta.json").read_text(encoding="utf-8"))

    print(json.dumps({"meta_status": meta.get("status"), **summary}, indent=2, default=str))

    assert meta["status"] == "finished", f"expected finished, got {meta['status']}"
    assert summary["equity_points"] > 0, "no equity points recorded"
