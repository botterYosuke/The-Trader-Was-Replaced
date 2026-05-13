from engine.jquants_loader import JQuantsLoader


def test_check_data_exists_returns_true_when_base_dir_exists(tmp_path):
    base_dir = tmp_path / "j-quants"
    base_dir.mkdir()

    loader = JQuantsLoader(str(base_dir))

    assert loader.check_data_exists(
        instrument_ids=["7203"],
        start_date="2024-01-01",
        end_date="2024-01-31",
    )


def test_check_data_exists_returns_false_when_base_dir_does_not_exist(tmp_path):
    base_dir = tmp_path / "missing-j-quants"

    loader = JQuantsLoader(str(base_dir))

    assert not loader.check_data_exists(
        instrument_ids=["7203"],
        start_date="2024-01-01",
        end_date="2024-01-31",
    )
