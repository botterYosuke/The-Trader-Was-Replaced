"""
Convert J-Quants CSV rows into Nautilus Bars and write them to a ParquetDataCatalog.

This is the bridge that lets the existing J-Quants CSV pipeline feed the new catalog-based
replay route. The conversion is one-way: J-Quants rows → Nautilus Bar objects → parquet on
disk. Replay reads the catalog back through `NautilusBarsReplayProvider`.

Entry points
------------
convert_daily_to_catalog / convert_minute_to_catalog
    Granularity-specific helpers; return the BarType string for backward compatibility.

ensure_jquants_catalog
    Preferred dispatcher for new call sites. Accepts granularity as a parameter and
    returns a JQuantsCatalogResult (catalog_path, bar_type, rows_written).

Calling DataEngine.load_replay_data() after conversion
-------------------------------------------------------
Pass only catalog_path — do NOT pass start_date / end_date. The CSV-to-catalog step
already applies the date filter. Passing date strings to catalog.query() causes Nautilus
to compare them against ns-precision ts_event, which returns an empty result.

Correct usage::

    result = ensure_jquants_catalog(base_dir, catalog_dir, "7203.TSE",
                                    "2024-07-01", "2024-07-31", "Daily")
    engine.load_replay_data(
        instrument_ids=[result.bar_type],
        granularity="Daily",
        catalog_path=result.catalog_path,
    )
"""

from dataclasses import dataclass
from pathlib import Path
from typing import Callable, List

from .jquants_loader import JQuantsLoader, daily_rows_to_ticks, minute_rows_to_ticks


@dataclass(frozen=True)
class JQuantsCatalogResult:
    catalog_path: str
    bar_type: str
    rows_written: int


def instrument_id_to_bar_type(instrument_id: str, granularity: str) -> str:
    """Map (instrument_id, granularity) to a Nautilus BarType string.

    Examples:
        ("7203.TSE", "Daily")  -> "7203.TSE-1-DAY-LAST-EXTERNAL"
        ("7203.TSE", "Minute") -> "7203.TSE-1-MINUTE-LAST-EXTERNAL"
    """
    agg = {"Daily": "1-DAY", "Minute": "1-MINUTE"}.get(granularity)
    if agg is None:
        raise ValueError(f"Unsupported granularity for BarType: {granularity!r}")
    return f"{instrument_id}-{agg}-LAST-EXTERNAL"


def _write_bars_to_catalog(
    rows: list,
    ticks_fn: Callable,
    bar_type_str: str,
    catalog_path: Path,
    price_precision: int,
) -> JQuantsCatalogResult:
    """Convert rows → Bars → write to ParquetDataCatalog. Returns JQuantsCatalogResult."""
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    ticks = ticks_fn(rows)
    bar_type = BarType.from_str(bar_type_str)

    def _price(v: float) -> Price:
        # Round-trip through a precision-formatted string so the Decimal backing is exact.
        return Price.from_str(f"{v:.{price_precision}f}")

    zero_volume = Quantity.from_int(0)

    bars: List[Bar] = []
    for ts_sec, o, h, l, c in ticks:
        ts_ns = int(ts_sec * 1e9)
        bars.append(
            Bar(
                bar_type=bar_type,
                open=_price(o),
                high=_price(h),
                low=_price(l),
                close=_price(c),
                volume=zero_volume,
                ts_event=ts_ns,
                ts_init=ts_ns,
            )
        )

    catalog_dir = catalog_path.resolve()
    catalog_dir.mkdir(parents=True, exist_ok=True)
    ParquetDataCatalog(str(catalog_dir)).write_data(bars)

    return JQuantsCatalogResult(
        catalog_path=str(catalog_dir),
        bar_type=bar_type_str,
        rows_written=len(bars),
    )


def convert_daily_to_catalog(
    base_dir: str | Path,
    catalog_path: str | Path,
    instrument_id: str,
    start_date: str,
    end_date: str,
    price_precision: int = 1,
) -> str:
    """
    Read J-Quants daily rows for `instrument_id` over [start_date, end_date], convert each
    to a Nautilus `Bar`, and write the batch into a `ParquetDataCatalog` at `catalog_path`.

    Returns the BarType string (for backward compatibility with existing call sites).
    Prefer `ensure_jquants_catalog` for new code.
    """
    loader = JQuantsLoader(str(base_dir))
    rows = loader.load_daily_rows(instrument_id, start_date, end_date)
    if not rows:
        raise ValueError(
            f"No daily rows for {instrument_id} {start_date}..{end_date} in {base_dir}"
        )
    bar_type_str = instrument_id_to_bar_type(instrument_id, "Daily")
    result = _write_bars_to_catalog(rows, daily_rows_to_ticks, bar_type_str, Path(catalog_path), price_precision)
    return result.bar_type


def convert_minute_to_catalog(
    base_dir: str | Path,
    catalog_path: str | Path,
    instrument_id: str,
    start_date: str,
    end_date: str,
    price_precision: int = 1,
) -> str:
    """
    Read J-Quants minute rows for `instrument_id` over [start_date, end_date], convert each
    to a Nautilus `Bar`, and write the batch into a `ParquetDataCatalog` at `catalog_path`.

    Returns the BarType string (for backward compatibility with existing call sites).
    Prefer `ensure_jquants_catalog` for new code.
    """
    loader = JQuantsLoader(str(base_dir))
    rows = loader.load_minute_rows(instrument_id, start_date, end_date)
    if not rows:
        raise ValueError(
            f"No minute rows for {instrument_id} {start_date}..{end_date} in {base_dir}"
        )
    bar_type_str = instrument_id_to_bar_type(instrument_id, "Minute")
    result = _write_bars_to_catalog(rows, minute_rows_to_ticks, bar_type_str, Path(catalog_path), price_precision)
    return result.bar_type


_GRANULARITY_CONFIG: dict[str, tuple[Callable, Callable]] = {
    "Daily": (
        lambda loader, inst, start, end: loader.load_daily_rows(inst, start, end),
        daily_rows_to_ticks,
    ),
    "Minute": (
        lambda loader, inst, start, end: loader.load_minute_rows(inst, start, end),
        minute_rows_to_ticks,
    ),
}


def ensure_jquants_catalog(
    base_dir: str | Path,
    catalog_path: str | Path,
    instrument_id: str,
    start_date: str,
    end_date: str,
    granularity: str,
    price_precision: int = 1,
) -> JQuantsCatalogResult:
    """
    Dispatcher: load J-Quants rows for the given granularity, convert to Nautilus Bars,
    and write to a ParquetDataCatalog.

    Returns JQuantsCatalogResult with the resolved catalog_path, bar_type (ready to pass
    directly to LoadReplayData instrument_ids), and rows_written.

    After calling this function, invoke DataEngine.load_replay_data() WITHOUT start/end::

        result = ensure_jquants_catalog(base_dir, catalog_dir, "7203.TSE",
                                        "2024-07-01", "2024-07-31", "Daily")
        engine.load_replay_data(
            instrument_ids=[result.bar_type],
            granularity="Daily",
            catalog_path=result.catalog_path,
        )
    """
    if granularity not in _GRANULARITY_CONFIG:
        raise ValueError(
            f"Unsupported granularity for catalog conversion: {granularity!r}. "
            f"Supported: {list(_GRANULARITY_CONFIG)}"
        )

    load_fn, ticks_fn = _GRANULARITY_CONFIG[granularity]
    loader = JQuantsLoader(str(base_dir))
    rows = load_fn(loader, instrument_id, start_date, end_date)
    if not rows:
        raise ValueError(
            f"No {granularity.lower()} rows for {instrument_id} "
            f"{start_date}..{end_date} in {base_dir}"
        )

    bar_type_str = instrument_id_to_bar_type(instrument_id, granularity)
    return _write_bars_to_catalog(rows, ticks_fn, bar_type_str, Path(catalog_path), price_precision)
