"""
Convert J-Quants daily CSV rows into Nautilus Bars and write them to a ParquetDataCatalog.

This is the bridge that lets the existing J-Quants CSV pipeline feed the new catalog-based
replay route. The conversion is one-way: J-Quants rows → Nautilus Bar objects → parquet on
disk. Replay reads the catalog back through `NautilusBarsReplayProvider`.

Daily only for the MVP. Minute is the obvious next horizontal step.
"""

from pathlib import Path
from typing import List

from .jquants_loader import JQuantsLoader, daily_rows_to_ticks, minute_rows_to_ticks


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

    Returns the BarType string used (so the caller can pass it to LoadReplayData).
    """
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    loader = JQuantsLoader(str(base_dir))
    rows = loader.load_daily_rows(instrument_id, start_date, end_date)
    if not rows:
        raise ValueError(
            f"No daily rows for {instrument_id} {start_date}..{end_date} in {base_dir}"
        )

    ticks = daily_rows_to_ticks(rows)
    bar_type_str = instrument_id_to_bar_type(instrument_id, "Daily")
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

    catalog_dir = Path(catalog_path)
    catalog_dir.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(catalog_dir.resolve()))
    catalog.write_data(bars)

    return bar_type_str


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

    Returns the BarType string used (so the caller can pass it to LoadReplayData).
    """
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    loader = JQuantsLoader(str(base_dir))
    rows = loader.load_minute_rows(instrument_id, start_date, end_date)
    if not rows:
        raise ValueError(
            f"No minute rows for {instrument_id} {start_date}..{end_date} in {base_dir}"
        )

    ticks = minute_rows_to_ticks(rows)
    bar_type_str = instrument_id_to_bar_type(instrument_id, "Minute")
    bar_type = BarType.from_str(bar_type_str)

    def _price(v: float) -> Price:
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

    catalog_dir = Path(catalog_path)
    catalog_dir.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(catalog_dir.resolve()))
    catalog.write_data(bars)

    return bar_type_str
