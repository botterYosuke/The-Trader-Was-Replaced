from pathlib import Path

import pytest

from engine.core import DataEngine
from engine.jquants_loader import JQuantsLoader

DATA_DIR = Path(__file__).parent / "data"


def test_data_engine_load_daily_jquants_primes_and_steps():
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader)

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )

    assert success, error
    assert engine.replay_state == "LOADED"
    assert engine.get_current_state().price == 3284.0

    assert engine.start_engine()[0]
    assert engine.pause_replay()[0]
    assert engine.step_replay()[0]
    assert engine.get_current_state().price == 3333.0


def test_data_engine_load_daily_exhausts_after_all_ticks():
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader)

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Daily",
    )
    engine.start_engine()
    engine.pause_replay()

    assert engine.get_current_state().price == 3284.0
    assert engine.is_exhausted


def test_data_engine_load_daily_rejects_second_load_while_loaded():
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader)

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )

    assert not success
    assert error is not None
    assert "IDLE" in error


def test_data_engine_load_minute_primes_and_steps(small_data_dir):
    loader = JQuantsLoader(str(small_data_dir))
    engine = DataEngine(jquants_loader=loader)

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Minute",
    )

    assert success, error
    assert engine.replay_state == "LOADED"
    assert engine.get_current_state().price == 3308.0

    assert engine.start_engine()[0]
    assert engine.pause_replay()[0]
    assert engine.step_replay()[0]
    assert engine.get_current_state().price == 3301.0


def test_data_engine_load_minute_exhausts_after_all_ticks(small_data_dir):
    loader = JQuantsLoader(str(small_data_dir))
    engine = DataEngine(jquants_loader=loader)

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Minute",
    )
    engine.start_engine()
    engine.pause_replay()

    # primed at tick 0 (3308.0); step through remaining 2 ticks
    engine.step_replay()  # 3301.0
    engine.step_replay()  # 3302.0
    assert engine.is_exhausted


def test_data_engine_load_minute_rejects_missing_data(tmp_path):
    loader = JQuantsLoader(str(tmp_path))
    engine = DataEngine(jquants_loader=loader)

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Minute",
    )

    assert not success
    assert error is not None
