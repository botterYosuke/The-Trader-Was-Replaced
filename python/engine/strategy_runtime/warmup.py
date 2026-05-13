"""engine.strategy_runtime.warmup — catalog-based WarmupLoader factory.

The blacksheep order_flow strategies accept a ``warmup_loader`` callback:

    warmup_loader(instrument_id: str, start_date: date, end_date: date)
        -> list[tuple[date, open, high, low, close, volume]]

The default in the strategy file calls ``engine.nautilus.jquants_loader``
(not available in this repo).  This module provides a drop-in replacement
that reads Daily bars from the Nautilus ParquetDataCatalog.

If the catalog has no Daily bars for a given instrument/window, an empty
list is returned — the strategy silently skips warmup for that instrument.

Public API:
    make_catalog_warmup_loader(catalog_path) -> WarmupLoader
"""
from __future__ import annotations

from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Callable

from engine.nautilus_catalog_loader import load_bars  # noqa: E402 (monkeypatchable at module scope)
from engine.jquants_to_catalog import instrument_id_to_bar_type

WarmupLoader = Callable[
    [str, date, date],
    list[tuple[date, float, float, float, float, float]],
]

_JST = timezone(timedelta(hours=9))


def make_catalog_warmup_loader(catalog_path: str | Path) -> WarmupLoader:
    """Return a WarmupLoader backed by a Nautilus ParquetDataCatalog.

    The loader reads **Daily** bars for a given symbol and date window.
    Any missing data (instrument not in catalog, date range outside
    coverage) returns an empty list rather than raising.

    Args:
        catalog_path: Root directory of the Nautilus catalog.

    Returns:
        A callable matching the WarmupLoader protocol.
    """
    _catalog = Path(catalog_path)

    def warmup_loader(
        instrument_id: str,
        start_date: date,
        end_date: date,
    ) -> list[tuple[date, float, float, float, float, float]]:
        bar_type = instrument_id_to_bar_type(instrument_id, "Daily")

        start_ns = int(
            datetime(start_date.year, start_date.month, start_date.day, tzinfo=timezone.utc).timestamp()
            * 1_000_000_000
        )
        end_ns = int(
            (
                datetime(end_date.year, end_date.month, end_date.day, tzinfo=timezone.utc)
                + timedelta(days=1)
            ).timestamp()
            * 1_000_000_000
        )

        try:
            bars = load_bars(_catalog, instrument_ids=[bar_type])
        except Exception:
            return []

        bars = [b for b in bars if start_ns <= b.ts_event < end_ns]
        if not bars:
            return []

        bars.sort(key=lambda b: b.ts_event)

        result: list[tuple[date, float, float, float, float, float]] = []
        for bar in bars:
            d = (
                datetime.fromtimestamp(bar.ts_event / 1_000_000_000, tz=timezone.utc)
                .astimezone(_JST)
                .date()
            )
            result.append((
                d,
                float(str(bar.open)),
                float(str(bar.high)),
                float(str(bar.low)),
                float(str(bar.close)),
                float(str(bar.volume)),
            ))

        return result

    return warmup_loader
