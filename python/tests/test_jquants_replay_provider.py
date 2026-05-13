import pytest
from pathlib import Path

from engine.jquants_loader import JQuantsLoader
from engine.replay import JQuantsDailyReplayProvider

DATA_DIR = Path(__file__).parents[0] / "data"


def test_jquants_daily_replay_provider_returns_ticks():
    loader = JQuantsLoader(str(DATA_DIR))
    provider = JQuantsDailyReplayProvider(
        loader=loader,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
    )

    assert provider.get_next_tick()[4] == 3284.0  # close is index 4
    assert provider.get_next_tick()[4] == 3333.0
    assert provider.get_next_tick() is None
    assert provider.is_exhausted()


def test_jquants_daily_replay_provider_timestamps_ascending():
    loader = JQuantsLoader(str(DATA_DIR))
    provider = JQuantsDailyReplayProvider(
        loader=loader,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
    )

    t1 = provider.get_next_tick()[0]
    t2 = provider.get_next_tick()[0]
    assert t1 < t2


def test_jquants_daily_replay_provider_current_index():
    loader = JQuantsLoader(str(DATA_DIR))
    provider = JQuantsDailyReplayProvider(
        loader=loader,
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
    )

    assert provider.current_index == 0
    provider.get_next_tick()
    assert provider.current_index == 1


def test_jquants_daily_replay_provider_raises_when_no_data(tmp_path):
    loader = JQuantsLoader(str(tmp_path))
    with pytest.raises(ValueError):
        JQuantsDailyReplayProvider(
            loader=loader,
            instrument_id="7203.TSE",
            start_date="2024-07-01",
            end_date="2024-07-31",
        )
