from pathlib import Path

import pytest

from engine.core import DataEngine
from engine.jquants_loader import JQuantsLoader
from engine.reducer import KlineUpdate, ReplayTimeUpdated

DATA_DIR = Path(__file__).parent / "data"


@pytest.mark.slow
def test_data_engine_load_daily_jquants_primes_and_steps(tmp_path):
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

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


@pytest.mark.slow
def test_data_engine_load_daily_exhausts_after_all_ticks(tmp_path):
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

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


# ---------------------------------------------------------------------------
# Catalog route fallback: ensure_jquants_catalog is called when catalog miss
# ---------------------------------------------------------------------------

def test_catalog_route_fallback_on_value_error(tmp_path):
    """NautilusBarsReplayProvider が ValueError を上げたとき、
    ensure_jquants_catalog を経由して再試行し成功することを確認する。"""
    from unittest.mock import MagicMock, patch

    jq_loader = MagicMock()
    jq_loader.base_dir = tmp_path / "jq"
    de = DataEngine(nautilus_catalog_path=str(tmp_path / "cat"), jquants_loader=jq_loader)

    fake_provider = MagicMock()
    fake_provider.get_next_tick.return_value = (1.0, 100.0, 105.0, 99.0, 103.0)
    fake_provider.is_exhausted.return_value = False

    with (
        patch(
            "engine.core.NautilusBarsReplayProvider",
            side_effect=[ValueError("no data"), fake_provider],
        ),
        patch("engine.core.ensure_jquants_catalog") as mock_ensure,
    ):
        success, error = de.load_replay_data(
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity="Minute",
        )

    assert success, error
    mock_ensure.assert_called_once()


def test_catalog_route_fallback_on_file_not_found_error(tmp_path):
    """NautilusBarsReplayProvider が FileNotFoundError を上げたときも
    ensure_jquants_catalog 経由の fallback が動くことを確認する。"""
    from unittest.mock import MagicMock, patch

    jq_loader = MagicMock()
    jq_loader.base_dir = tmp_path / "jq"
    de = DataEngine(nautilus_catalog_path=str(tmp_path / "cat"), jquants_loader=jq_loader)

    fake_provider = MagicMock()
    fake_provider.get_next_tick.return_value = (1.0, 100.0, 105.0, 99.0, 103.0)
    fake_provider.is_exhausted.return_value = False

    with (
        patch(
            "engine.core.NautilusBarsReplayProvider",
            side_effect=[FileNotFoundError("missing parquet"), fake_provider],
        ),
        patch("engine.core.ensure_jquants_catalog") as mock_ensure,
    ):
        success, error = de.load_replay_data(
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity="Minute",
        )

    assert success, error
    mock_ensure.assert_called_once()


def test_catalog_route_no_fallback_when_provider_succeeds(tmp_path):
    """NautilusBarsReplayProvider が成功した場合は ensure_jquants_catalog を呼ばない。"""
    from unittest.mock import MagicMock, patch

    engine = DataEngine(nautilus_catalog_path=str(tmp_path / "cat"))

    fake_provider = MagicMock()
    fake_provider.get_next_tick.return_value = (1.0, 100.0, 105.0, 99.0, 103.0)
    fake_provider.is_exhausted.return_value = False

    with (
        patch(
            "engine.core.NautilusBarsReplayProvider",
            return_value=fake_provider,
        ),
        patch(
            "engine.core.ensure_jquants_catalog",
        ) as mock_ensure,
    ):
        success, error = engine.load_replay_data(
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity="Minute",
        )

    assert success, error
    mock_ensure.assert_not_called()


def test_catalog_route_no_fallback_when_no_jquants_loader(tmp_path):
    """jquants_loader が無い場合、catalog miss 時に fallback せず即 False を返す。"""
    from unittest.mock import MagicMock, patch

    de = DataEngine(nautilus_catalog_path=str(tmp_path / "cat"))  # no jquants_loader

    with (
        patch(
            "engine.core.NautilusBarsReplayProvider",
            side_effect=ValueError("no data"),
        ),
        patch("engine.core.ensure_jquants_catalog") as mock_ensure,
    ):
        success, error = de.load_replay_data(
            instrument_ids=["7203.TSE"],
            start_date="2024-07-01",
            end_date="2024-07-01",
            granularity="Minute",
        )

    assert not success
    mock_ensure.assert_not_called()


@pytest.mark.slow
def test_data_engine_load_daily_rejects_second_load_while_loaded(tmp_path):
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

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


@pytest.mark.slow
def test_data_engine_load_minute_primes_and_steps(small_data_dir, tmp_path):
    loader = JQuantsLoader(str(small_data_dir))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

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


@pytest.mark.slow
def test_data_engine_load_minute_exhausts_after_all_ticks(small_data_dir, tmp_path):
    loader = JQuantsLoader(str(small_data_dir))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

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
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Minute",
    )

    assert not success
    assert error is not None


@pytest.mark.slow
def test_step_replay_fires_time_updated_then_kline_update(tmp_path):
    """
    StepReplay が ReplayTimeUpdated -> KlineUpdate の順でイベントを発火することを確認する。
    この順序は Phase 6 計画の「TimeUpdated -> DataUpdated」の契約を保証する。
    """
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )
    engine.start_engine()
    engine.pause_replay()

    log_before = len(engine.get_event_log())
    engine.step_replay()

    new_events = engine.get_event_log()[log_before:]
    assert len(new_events) == 2
    assert isinstance(new_events[0], ReplayTimeUpdated)
    assert isinstance(new_events[1], KlineUpdate)
    assert new_events[0].timestamp_ms == new_events[1].timestamp_ms


@pytest.mark.slow
def test_event_log_accumulates_across_steps(tmp_path):
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
        granularity="Daily",
    )
    engine.start_engine()
    engine.pause_replay()

    engine.step_replay()
    assert len(engine.get_event_log()) == 2  # 1 step = ReplayTimeUpdated + KlineUpdate

    engine.step_replay()
    assert len(engine.get_event_log()) == 4  # 2 steps accumulated


@pytest.mark.slow
def test_get_current_state_includes_ohlc_after_step(tmp_path):
    """StepReplay 後の get_current_state が OHLC フィールドを持つことを確認する。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )
    engine.start_engine()
    engine.pause_replay()
    engine.step_replay()

    state = engine.get_current_state()
    assert state.close == state.price
    assert state.open is not None
    assert state.high is not None
    assert state.low is not None
    assert state.high >= state.low
    assert state.high >= state.close
    assert state.low <= state.close


@pytest.mark.slow
def test_get_current_state_ohlc_exact_values_from_daily_data(tmp_path):
    """Daily データの OHLC exact 値を GetState から確認する。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    # 2024-07-01 の J-Quants 実データ: O=3325.0 H=3326.0 L=3261.0 C=3284.0
    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-01",
        granularity="Daily",
    )

    state = engine.get_current_state()
    assert state.price == 3284.0
    assert state.close == 3284.0
    assert state.open == 3325.0
    assert state.high == 3326.0
    assert state.low == 3261.0


def test_get_current_state_ohlc_none_in_static_mode():
    """Static モードでは OHLC は None になる。"""
    engine = DataEngine()
    state = engine.get_current_state()
    assert state.open is None
    assert state.high is None
    assert state.low is None


def test_load_replay_data_rejects_missing_catalog_path():
    """jquants_loader があっても jquants_catalog_path が None なら Daily/Minute は fail する。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader)  # no jquants_catalog_path

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
        granularity="Daily",
    )

    assert not success
    assert error is not None
    assert "catalog path" in error.lower()
    assert engine.replay_state == "IDLE"


def test_load_replay_data_rejects_trade_granularity():
    """Trade granularity は Phase 6 MVP 未対応として明示 reject される。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader)

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
        granularity="Trade",
    )

    assert not success
    assert error is not None
    assert "Trade" in error
    assert engine.replay_state == "IDLE"


def test_load_replay_data_rejects_unknown_granularity():
    """未知の granularity も reject される。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader)

    success, error = engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
        granularity="Tick",
    )

    assert not success
    assert error is not None
    assert engine.replay_state == "IDLE"


@pytest.mark.slow
def test_prime_does_not_emit_replay_events(tmp_path):
    """_prime_provider_locked は event_log にイベントを追加しない。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )

    assert engine.get_event_log() == []


@pytest.mark.slow
def test_step_replay_kline_update_has_ohlc(tmp_path):
    """StepReplay で発火する KlineUpdate が OHLC フィールドを持つことを確認する。"""
    loader = JQuantsLoader(str(DATA_DIR))
    engine = DataEngine(jquants_loader=loader, jquants_catalog_path=str(tmp_path / "cat"))

    engine.load_replay_data(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-02",
        granularity="Daily",
    )
    engine.start_engine()
    engine.pause_replay()
    engine.step_replay()

    kline = next(e for e in engine.get_event_log() if isinstance(e, KlineUpdate))
    assert kline.close > 0
    assert kline.open > 0
    assert kline.high >= kline.low
    assert kline.high >= kline.close
    assert kline.low <= kline.close
