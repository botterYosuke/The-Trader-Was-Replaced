from engine.jquants_loader import JQuantsLoader

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
