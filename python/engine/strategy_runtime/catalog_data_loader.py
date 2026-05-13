"""engine.strategy_runtime.catalog_data_loader — SCENARIO → Bar リスト変換。

Phase 6 の ParquetDataCatalog 資産を BacktestEngine へのデータ供給口として使う。

Public API:
    instruments_from_scenario(scenario)                 -> list[str]
    normalize_granularity(value)                        -> str
    bar_type_for_instrument(instrument, granularity)    -> str
    load_bars_for_scenario(catalog_path, scenario)      -> dict[InstrumentId, list[Bar]]
    merge_bars_by_ts(bars_by_instrument)                -> list[Bar]
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from engine.nautilus_catalog_loader import load_bars  # noqa: E402  (import at module scope for monkeypatching)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def instruments_from_scenario(scenario: dict) -> list[str]:  # type: ignore[type-arg]
    """SCENARIO から銘柄リストを返す（v1/v2/v3 共通）。"""
    if "instrument" in scenario:
        return [scenario["instrument"]]
    return list(scenario["instruments"])


def normalize_granularity(value: str) -> str:
    """大文字小文字・前後空白を正規化して "Daily" または "Minute" を返す。

    Raises:
        ValueError: "daily" / "minute" 以外の場合。
    """
    v = value.strip().lower()
    if v == "daily":
        return "Daily"
    if v == "minute":
        return "Minute"
    raise ValueError(f"Unsupported granularity: {value!r}")


def bar_type_for_instrument(instrument: str, granularity: str) -> str:
    """(instrument_id, 正規化済み granularity) → BarType 文字列。

    >>> bar_type_for_instrument("1301.TSE", "Minute")
    '1301.TSE-1-MINUTE-LAST-EXTERNAL'
    """
    from engine.jquants_to_catalog import instrument_id_to_bar_type

    return instrument_id_to_bar_type(instrument, granularity)


# ---------------------------------------------------------------------------
# Date → nanosecond timestamp helpers
# ---------------------------------------------------------------------------


def _date_str_to_ns(date_str: str) -> int:
    """'YYYY-MM-DD' → UTC midnight in nanoseconds."""
    dt = datetime.fromisoformat(date_str).replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1_000_000_000)


def _date_str_to_exclusive_end_ns(date_str: str) -> int:
    """'YYYY-MM-DD' → UTC midnight of *next* day in nanoseconds (exclusive upper bound)."""
    dt = (datetime.fromisoformat(date_str) + timedelta(days=1)).replace(
        tzinfo=timezone.utc
    )
    return int(dt.timestamp() * 1_000_000_000)


# ---------------------------------------------------------------------------
# Core loader
# ---------------------------------------------------------------------------


def load_bars_for_scenario(
    catalog_path: str | Path,
    scenario: dict,  # type: ignore[type-arg]
) -> dict[Any, list]:  # dict[InstrumentId, list[Bar]]
    """SCENARIO に従い catalog から Bar を読み込み、銘柄別 dict を返す。

    各銘柄のリストは ts_event 昇順でソート済み。
    SCENARIO の start/end で Python 側フィルタも掛ける（念のため）。

    Returns:
        {InstrumentId: list[Bar]} — 各リストは ts_event 昇順。
    """
    from nautilus_trader.model.identifiers import InstrumentId

    granularity = normalize_granularity(scenario["granularity"])
    start_ns = _date_str_to_ns(scenario["start"])
    end_ns = _date_str_to_exclusive_end_ns(scenario["end"])

    result: dict[Any, list] = {}

    for symbol in instruments_from_scenario(scenario):
        bar_type = bar_type_for_instrument(symbol, granularity)
        bars = load_bars(catalog_path, instrument_ids=[bar_type])

        # Python 側 ts_event フィルタ
        bars = [b for b in bars if start_ns <= b.ts_event < end_ns]
        bars.sort(key=lambda b: b.ts_event)

        instrument_id = InstrumentId.from_str(symbol)
        result[instrument_id] = bars

    return result


# ---------------------------------------------------------------------------
# Merge helper
# ---------------------------------------------------------------------------


def merge_bars_by_ts(bars_by_instrument: dict) -> list:  # list[Bar]
    """複数銘柄の Bar を ts_event 昇順にマージした単一リストを返す。

    同一 ts_event の場合の順序は stable sort により元の銘柄順を保持する。
    e-station engine_runner と同じ streaming 順を再現するために使う。
    """
    all_bars: list = []
    for bars in bars_by_instrument.values():
        all_bars.extend(bars)
    all_bars.sort(key=lambda b: b.ts_event)
    return all_bars
