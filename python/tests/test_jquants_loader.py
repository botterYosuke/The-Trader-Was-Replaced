import pytest

from engine.jquants_loader import (
    JQuantsLoader,
    daily_rows_to_ticks,
    instrument_id_to_jquants_code,
    minute_rows_to_ticks,
)

from pathlib import Path

SAMPLE_DATA = Path(__file__).parents[0] / "data"


def test_check_data_exists_returns_true_when_monthly_trade_file_exists(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()
    (base_dir / "equities_trades_202401.csv.gz").write_text("")

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203.TSE"],
        start_date="2024-01-01",
        end_date="2024-01-31",
    )


def test_check_data_exists_returns_false_when_monthly_file_is_missing(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()

    loader = JQuantsLoader(str(base_dir))

    assert not loader.check_data_exists(
        instrument_ids=["7203.TSE"],
        start_date="2024-01-01",
        end_date="2024-01-31",
    )


def test_check_data_exists_checks_month_boundary(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()
    (base_dir / "equities_trades_202402.csv.gz").write_text("")

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203.TSE"],
        start_date="2024-01-31",
        end_date="2024-02-01",
    )


def test_check_data_exists_accepts_multiple_instrument_ids(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()
    (base_dir / "equities_trades_202407.csv.gz").write_text("")

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203.TSE", "6758.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
    )


def test_check_data_exists_with_sample_trade_data():
    loader = JQuantsLoader(str(SAMPLE_DATA))

    assert loader.check_data_exists(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-08-31",
    )


def test_check_data_exists_supports_minute_granularity(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()
    (base_dir / "equities_bars_minute_202407.csv.gz").write_text("")

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
        granularity="Minute",
    )


def test_check_data_exists_supports_daily_granularity(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()
    (base_dir / "equities_bars_daily_202407.csv.gz").write_text("")

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203.TSE"],
        start_date="2024-07-01",
        end_date="2024-07-31",
        granularity="Daily",
    )


def test_instrument_id_to_jquants_code():
    assert instrument_id_to_jquants_code("7203.TSE") == "72030"
    assert instrument_id_to_jquants_code("1301.TSE") == "13010"


def test_instrument_id_to_jquants_code_no_exchange_suffix():
    assert instrument_id_to_jquants_code("7203") == "72030"


def test_instrument_id_to_jquants_code_rejects_empty_symbol():
    with pytest.raises(ValueError):
        instrument_id_to_jquants_code(".TSE")


def test_load_daily_rows_filters_by_instrument_and_date():
    loader = JQuantsLoader(str(SAMPLE_DATA))

    rows = loader.load_daily_rows(
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-02",
    )

    assert [row["Date"] for row in rows] == ["2024-07-01", "2024-07-02"]
    assert {row["Code"] for row in rows} == {"72030"}
    assert rows[0]["C"] == "3284.0"
    assert rows[1]["C"] == "3333.0"


def test_load_daily_rows_returns_empty_when_no_file(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()
    loader = JQuantsLoader(str(base_dir))

    rows = loader.load_daily_rows(
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-31",
    )

    assert rows == []


def test_daily_rows_to_ticks_uses_date_and_close():
    rows = [
        {"Date": "2024-07-01", "Code": "72030", "O": "3280.0", "H": "3290.0", "L": "3275.0", "C": "3284.0"},
        {"Date": "2024-07-02", "Code": "72030", "O": "3330.0", "H": "3340.0", "L": "3320.0", "C": "3333.0"},
    ]

    ticks = daily_rows_to_ticks(rows)

    assert [t[4] for t in ticks] == [3284.0, 3333.0]  # close is index 4
    assert ticks[0][1] == 3280.0  # open
    assert ticks[0][2] == 3290.0  # high
    assert ticks[0][3] == 3275.0  # low
    assert ticks[0][0] < ticks[1][0]


def test_daily_rows_to_ticks_timestamps_are_jst_1530():
    from zoneinfo import ZoneInfo
    from datetime import datetime

    rows = [{"Date": "2024-07-01", "Code": "72030", "O": "3280.0", "H": "3290.0", "L": "3275.0", "C": "3284.0"}]
    ticks = daily_rows_to_ticks(rows)

    ts = ticks[0][0]
    dt = datetime.fromtimestamp(ts, tz=ZoneInfo("Asia/Tokyo"))
    assert dt.hour == 15
    assert dt.minute == 30
    assert dt.date().isoformat() == "2024-07-01"


def test_daily_rows_to_ticks_returns_empty_for_empty_input():
    assert daily_rows_to_ticks([]) == []


def test_load_daily_rows_excludes_other_instruments():
    loader = JQuantsLoader(str(SAMPLE_DATA))

    rows = loader.load_daily_rows(
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-31",
    )

    codes = {row["Code"] for row in rows}
    assert codes == {"72030"}


@pytest.mark.slow
def test_load_minute_rows_filters_by_instrument_and_date():
    loader = JQuantsLoader(str(SAMPLE_DATA))

    rows = loader.load_minute_rows(
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-01",
    )

    assert len(rows) == 302
    assert {row["Code"] for row in rows} == {"72030"}
    assert rows[0]["Time"] == "09:00"
    assert rows[0]["C"] == "3308"
    assert rows[1]["Time"] == "09:01"
    assert rows[1]["C"] == "3301"


def test_load_minute_rows_returns_empty_when_no_file(tmp_path):
    loader = JQuantsLoader(str(tmp_path))

    rows = loader.load_minute_rows(
        instrument_id="7203.TSE",
        start_date="2024-07-01",
        end_date="2024-07-31",
    )

    assert rows == []


def test_minute_rows_to_ticks_uses_close_time_jst():
    from datetime import datetime
    from zoneinfo import ZoneInfo

    rows = [
        {"Date": "2024-07-01", "Time": "09:00", "Code": "72030", "O": "3310", "H": "3315", "L": "3305", "C": "3308"},
        {"Date": "2024-07-01", "Time": "09:01", "Code": "72030", "O": "3305", "H": "3310", "L": "3298", "C": "3301"},
    ]

    ticks = minute_rows_to_ticks(rows)

    assert [t[4] for t in ticks] == [3308.0, 3301.0]  # close is index 4
    assert ticks[0][1] == 3310.0  # open
    assert ticks[0][2] == 3315.0  # high
    assert ticks[0][3] == 3305.0  # low
    assert ticks[0][0] < ticks[1][0]

    dt = datetime.fromtimestamp(ticks[0][0], tz=ZoneInfo("Asia/Tokyo"))
    assert dt.hour == 9
    assert dt.minute == 0
    assert dt.second == 59
    assert dt.microsecond == 999999


def test_minute_rows_to_ticks_returns_empty_for_empty_input():
    assert minute_rows_to_ticks([]) == []
