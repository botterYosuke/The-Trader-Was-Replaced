"""conftest.py — slow テスト用カタログ自動構築フィクスチャ。

カタログに必要な Minute バーが存在しない場合、利用可能な J-Quants CSV
ディレクトリから自動的に構築する。どのデータソースも見つからない場合は
pytest.skip() で該当テストをスキップする。
"""
from __future__ import annotations

from pathlib import Path

import pytest

from engine.paths import jquants_catalog_path as _jquants_catalog_path

_REPO_ROOT = Path(__file__).parents[3]
_CATALOG_PATH = _jquants_catalog_path()

# J-Quants データソース候補（先頭から順に試す）
_JQUANTS_DATA_DIRS = [
    Path("S:/j-quants"),
    _REPO_ROOT / "examples",
]

# slow テストが必要とする (instrument_id, granularity, start, end) の一覧
_REQUIRED_BARS = [
    ("1301.TSE", "Minute", "2025-01-06", "2025-01-10"),
]


def _minute_bar_dir(catalog_path: Path, instrument_id: str) -> Path:
    return catalog_path / "data" / "bar" / f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"


def _has_bars(catalog_path: Path, instrument_id: str) -> bool:
    d = _minute_bar_dir(catalog_path, instrument_id)
    return d.exists() and any(d.glob("*.parquet"))


@pytest.fixture(scope="session")
def ensure_slow_catalog():
    """slow テストが必要とする Minute バーをカタログに保証する。

    - バーが存在すれば何もしない。
    - 存在しなければ _JQUANTS_DATA_DIRS を順に試して構築する。
    - どのデータソースも使えなければ pytest.skip() する。
    """
    from engine.jquants_to_catalog import ensure_jquants_catalog

    for instrument_id, granularity, start_date, end_date in _REQUIRED_BARS:
        if _has_bars(_CATALOG_PATH, instrument_id):
            continue

        built = False
        last_error: str = ""
        for data_dir in _JQUANTS_DATA_DIRS:
            if not data_dir.exists():
                continue
            try:
                result = ensure_jquants_catalog(
                    base_dir=data_dir,
                    catalog_path=_CATALOG_PATH,
                    instrument_id=instrument_id,
                    start_date=start_date,
                    end_date=end_date,
                    granularity=granularity,
                )
                if result.rows_written > 0:
                    built = True
                    break
            except Exception as exc:
                last_error = str(exc)

        if not built:
            tried = [str(d) for d in _JQUANTS_DATA_DIRS]
            pytest.skip(
                f"Cannot build {granularity} catalog for {instrument_id} "
                f"({start_date}..{end_date}). "
                f"Tried data dirs: {tried}. Last error: {last_error}"
            )
