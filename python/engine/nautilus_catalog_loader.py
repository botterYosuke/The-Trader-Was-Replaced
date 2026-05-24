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

import glob
import os
from pathlib import Path
from typing import Any, Optional


class CatalogPrecisionMismatchError(ValueError):
    """The catalog parquet was written with a different nautilus precision build
    than the one currently running.

    nautilus bakes its precision mode (standard 8-byte / high 16-byte) into the
    compiled wheel via the ``HIGH_PRECISION`` Cargo feature. When ``catalog.query()``
    hits a ``fixed_size_binary`` width it wasn't compiled for, nautilus ``.unwrap()``s
    a ``PrecisionMismatch`` deep inside Rust and **aborts the whole backend process**
    (SIGABRT) — uncatchable from Python, and the UI only ever sees "transport error".

    This loader runs a hard preflight *before* ``query()`` and raises this instead, so
    the real cause reaches the UI. It subclasses ``ValueError`` so the existing
    ``except (ValueError, FileNotFoundError)`` handlers in the replay/StartEngine paths
    surface its message rather than letting it escape as an opaque gRPC error.

    See GH #34.
    """


def _running_precision_bytes() -> int:
    """PRECISION_BYTES of the currently-installed nautilus wheel (8 or 16)."""
    from nautilus_trader.core import nautilus_pyo3

    return int(nautilus_pyo3.PRECISION_BYTES)


def _assert_catalog_precision_compatible(
    catalog_root: str | Path,
    data_subdir: str,
    identifiers: Optional[list[str]],
    expected_bytes: Optional[int] = None,
) -> None:
    """Hard gate run *before* ``catalog.query()``.

    Inspect the ``fixed_size_binary`` column width of the parquet files ``query()``
    is about to read (``<root>/data/<subdir>/<identifier>/*.parquet``) and compare it
    to the running nautilus ``PRECISION_BYTES``. Raise :class:`CatalogPrecisionMismatchError`
    on any mismatch.

    No-op when the catalog has no matching parquet files (width is then unknowable and
    ``query()`` will report "no data" cleanly). ``expected_bytes`` is injectable for
    testing; it defaults to the running build.
    """
    import pyarrow as pa
    import pyarrow.parquet as pq

    data_dir = Path(catalog_root) / "data" / data_subdir
    if not data_dir.exists():
        return

    if identifiers:
        id_dirs: list[Path] = []
        for ident in identifiers:
            exact = data_dir / ident
            if exact.is_dir():
                id_dirs.append(exact)
            else:
                # nautilus prefix-matches identifiers: query(identifiers=["1301.TSE"])
                # reads dir "1301.TSE-1-MINUTE-LAST-EXTERNAL". Exact-dir-only matching
                # would no-op for a bare instrument id and let an unchecked width reach
                # query() (→ abort). Over-matching is safe — we only inspect more files,
                # all written by the same build.
                id_dirs.extend(
                    p for p in data_dir.iterdir() if p.is_dir() and p.name.startswith(ident)
                )
    else:
        id_dirs = [p for p in data_dir.iterdir() if p.is_dir()]

    # Check every parquet file query() could read. A single 8-byte shard among 16-byte
    # files would still abort the process if it reached query(), so sampling one file
    # per identifier is not enough. read_schema reads only the footer (cheap).
    files: list[str] = []
    for id_dir in id_dirs:
        files.extend(sorted(glob.glob(str(id_dir / "*.parquet"))))
    if not files:
        return

    if expected_bytes is None:
        expected_bytes = _running_precision_bytes()

    for f in files:
        schema = pq.read_schema(f)
        for name in schema.names:
            field_type = schema.field(name).type
            if pa.types.is_fixed_size_binary(field_type):
                width = field_type.byte_width
                if width != expected_bytes:
                    raise CatalogPrecisionMismatchError(
                        f"Catalog precision mismatch: {f} stores fixed_size_binary[{width}] "
                        f"for column {name!r}, but the running nautilus build expects "
                        f"PRECISION_BYTES={expected_bytes}. The shared catalog is "
                        f"standard-precision (8-byte) while this machine's nautilus is "
                        f"high-precision (16-byte). Rebuild nautilus standard-precision "
                        f"(HIGH_PRECISION=false — see scripts/rebuild_nautilus_standard.sh) "
                        f"to match the catalog; do NOT rewrite the shared catalog. (GH #34)"
                    )


def _resolve_catalog_path(catalog_path: str | Path) -> str:
    raw = os.fspath(catalog_path)
    # UNC paths become file://host/... in DataFusion, which has no ObjectStore
    # for the host component -> "No suitable object store found". Map the share
    # to a drive letter and pass that instead.
    if raw.startswith("\\\\") or raw.startswith("//"):
        raise ValueError(
            f"UNC catalog paths are not supported (got {raw!r}). "
            "Map the share to a drive letter (e.g. S:) and pass that instead."
        )
    # NOTE: Path.resolve() on Windows walks reparse points and rewrites
    # mapped drives (S:\) back to their UNC form (\\host\share\...). Use
    # absolute() -- it only prepends CWD when the path is relative.
    p = Path(raw).absolute()
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

    root = _resolve_catalog_path(catalog_path)
    _assert_catalog_precision_compatible(root, "bar", instrument_ids)
    catalog = ParquetDataCatalog(root)
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

    root = _resolve_catalog_path(catalog_path)
    _assert_catalog_precision_compatible(root, "trade_tick", instrument_ids)
    catalog = ParquetDataCatalog(root)
    return catalog.query(
        data_cls=TradeTick,
        identifiers=instrument_ids,
        start=start,
        end=end,
    )
