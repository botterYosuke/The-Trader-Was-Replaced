"""
Thin wrapper around ParquetDataCatalog for the project's replay pipeline.

The real catalog API is:
    catalog.query(data_cls=<Bar|TradeTick>, identifiers=..., start=..., end=...)
                                                            -- confirmed against
    .claude/skills/nautilus_trader/src/nautilus_trader/persistence/catalog/parquet.py:1576

There is no `catalog.bars()` / `catalog.trade_ticks()` shortcut — `query()` with a data class
is the canonical entry point. This loader names the two cases we care about so call sites
read clearly:

    runner.run_bars(load_bars(catalog_path, instrument_ids=[...]))
"""

from pathlib import Path
from typing import Any, Optional


def _resolve_catalog_path(catalog_path: str | Path) -> str:
    # ParquetDataCatalog requires an existing absolute path.
    p = Path(catalog_path).resolve()
    if not p.exists():
        raise FileNotFoundError(f"Catalog path does not exist: {p}")
    return str(p)


def load_bars(
    catalog_path: str | Path,
    instrument_ids: Optional[list[str]] = None,
    start: Any = None,
    end: Any = None,
) -> list:
    """Return all Bars in the catalog matching the filters, in catalog order."""
    from nautilus_trader.model.data import Bar
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    catalog = ParquetDataCatalog(_resolve_catalog_path(catalog_path))
    return catalog.query(
        data_cls=Bar,
        identifiers=instrument_ids,
        start=start,
        end=end,
    )


def load_trades(
    catalog_path: str | Path,
    instrument_ids: Optional[list[str]] = None,
    start: Any = None,
    end: Any = None,
) -> list:
    """Return all TradeTicks in the catalog matching the filters, in catalog order."""
    from nautilus_trader.model.data import TradeTick
    from nautilus_trader.persistence.catalog import ParquetDataCatalog

    catalog = ParquetDataCatalog(_resolve_catalog_path(catalog_path))
    return catalog.query(
        data_cls=TradeTick,
        identifiers=instrument_ids,
        start=start,
        end=end,
    )
