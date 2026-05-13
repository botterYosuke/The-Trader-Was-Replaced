import gzip

import pytest

_SMALL_MINUTE_CSV = (
    "Date,Time,Code,O,H,L,C,Vo,Va\n"
    "2024-07-01,09:00,72030,3325,3326,3303,3308,1676700,5571011000\n"
    "2024-07-01,09:01,72030,3309,3310,3298,3301,259800,857951000\n"
    "2024-07-01,09:02,72030,3300,3305,3295,3302,100000,330000000\n"
)

_SMALL_DAILY_CSV = (
    "Date,Code,O,H,L,C,UL,LL,Vo,Va,AdjFactor\n"
    "2024-07-01,72030,3325.0,3326.0,3261.0,3284.0,0,0,23624800.0,77575771100.0,1.0\n"
    "2024-07-02,72030,3285.0,3338.0,3267.0,3333.0,0,0,30146300.0,99970905000.0,1.0\n"
)


@pytest.fixture
def small_data_dir(tmp_path):
    (tmp_path / "equities_bars_minute_202407.csv.gz").write_bytes(
        gzip.compress(_SMALL_MINUTE_CSV.encode())
    )
    (tmp_path / "equities_bars_daily_202407.csv.gz").write_bytes(
        gzip.compress(_SMALL_DAILY_CSV.encode())
    )
    return tmp_path
