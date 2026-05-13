"""
Parity tests: legacy JQuants direct provider vs Nautilus catalog route.

Both routes must produce identical prime and step-one values.
After the migration of DataEngine to catalog-only route, _run_direct uses the
legacy JQuantsDailyReplayProvider / JQuantsMinuteReplayProvider directly (not
through DataEngine) so the comparison remains meaningful.
"""

import os

import pytest

from engine.jquants_loader import JQuantsLoader
from engine.jquants_to_catalog import convert_daily_to_catalog, convert_minute_to_catalog
from engine.core import DataEngine

DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")


def _run_direct(granularity: str, start: str, end: str) -> tuple[float, float]:
    """Run legacy JQuants provider directly; return (prime_price, step_one_price)."""
    from engine.replay import JQuantsDailyReplayProvider, JQuantsMinuteReplayProvider

    loader = JQuantsLoader(DATA_DIR)
    ProviderCls = (
        JQuantsDailyReplayProvider if granularity == "Daily" else JQuantsMinuteReplayProvider
    )
    provider = ProviderCls(
        loader=loader,
        instrument_id="7203.TSE",
        start_date=start,
        end_date=end,
    )

    prime_tick = provider.get_next_tick()
    assert prime_tick is not None
    prime = prime_tick[4]  # close

    step_tick = provider.get_next_tick()
    assert step_tick is not None
    step_one = step_tick[4]  # close

    return prime, step_one


def _run_catalog(
    granularity: str, start: str, end: str, tmp_path
) -> tuple[float, float]:
    """Run catalog route; return (prime_price, step_one_price)."""
    catalog_dir = tmp_path / "catalog"

    convert_fn = convert_daily_to_catalog if granularity == "Daily" else convert_minute_to_catalog
    bar_type_str = convert_fn(
        base_dir=DATA_DIR,
        catalog_path=catalog_dir,
        instrument_id="7203.TSE",
        start_date=start,
        end_date=end,
    )

    engine = DataEngine()
    ok, err = engine.load_replay_data(
        instrument_ids=[bar_type_str],
        granularity=granularity,
        catalog_path=str(catalog_dir),
    )
    assert ok, err

    prime = engine.get_current_state().price

    engine.start_engine()
    engine.pause_replay()
    engine.step_replay()

    step_one = engine.get_current_state().price
    return prime, step_one


@pytest.mark.slow
def test_daily_parity_prime_and_step(tmp_path):
    direct_prime, direct_step = _run_direct("Daily", "2024-07-01", "2024-07-02")
    catalog_prime, catalog_step = _run_catalog("Daily", "2024-07-01", "2024-07-02", tmp_path)

    assert catalog_prime == direct_prime, (
        f"Prime mismatch: direct={direct_prime}, catalog={catalog_prime}"
    )
    assert catalog_step == direct_step, (
        f"Step-one mismatch: direct={direct_step}, catalog={catalog_step}"
    )


@pytest.mark.slow
def test_minute_parity_prime_and_step(tmp_path):
    direct_prime, direct_step = _run_direct("Minute", "2024-07-01", "2024-07-01")
    catalog_prime, catalog_step = _run_catalog("Minute", "2024-07-01", "2024-07-01", tmp_path)

    assert catalog_prime == direct_prime, (
        f"Prime mismatch: direct={direct_prime}, catalog={catalog_prime}"
    )
    assert catalog_step == direct_step, (
        f"Step-one mismatch: direct={direct_step}, catalog={catalog_step}"
    )
