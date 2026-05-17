"""
Batch catalog builder — reads each CSV.gz file once and writes all instruments.
Builds both Minute and Daily bars with actual volume (Vo column).
Skips instruments that already have correct data in the catalog.

Usage:
    uv run python scripts/build_catalog_batch.py [--force]
"""
from __future__ import annotations

import argparse
import csv
import gzip
import json
import sys
import time
from datetime import date, datetime, time as dt_time
from pathlib import Path
from zoneinfo import ZoneInfo

_JST = ZoneInfo("Asia/Tokyo")

from engine.paths import jquants_cache_dir, jquants_catalog_path

BASE_DIR = jquants_cache_dir()
if BASE_DIR is None:
    raise RuntimeError("DEV_J_QUANTS_CACHE is not set")
CATALOG = jquants_catalog_path()
MINUTE_START = "2024-11-01"
MINUTE_END   = "2025-01-30"
DAILY_START  = "2024-10-01"   # warmup needs ~47 trading days before Jan 06
DAILY_END    = "2025-01-12"   # day before jan0610 scenario start

UNIVERSE_JSONS = [
    Path(r"D:\Documents\_blacksheep\data\universe\v05_B_top100_jan1324.json"),
    Path(r"D:\Documents\_blacksheep\data\universe\v05_B_top100_jan0610.json"),
]


def _iter_yyyymm(start: str, end: str):
    s = date.fromisoformat(start)
    e = date.fromisoformat(end)
    y, m = s.year, s.month
    while (y, m) <= (e.year, e.month):
        yield f"{y:04d}{m:02d}"
        if m == 12:
            y += 1
            m = 1
        else:
            m += 1


def _instrument_to_code(iid: str) -> str:
    return iid.split(".", 1)[0] + "0"


def _bar_dir(iid: str, gran: str) -> Path:
    suffix = "1-MINUTE-LAST-EXTERNAL" if gran == "Minute" else "1-DAY-LAST-EXTERNAL"
    return CATALOG / "data" / "bar" / f"{iid}-{suffix}"


def _needs_build(iid: str, gran: str) -> bool:
    d = _bar_dir(iid, gran)
    return not (d.exists() and any(d.iterdir()))


def _load_all_instruments() -> list[str]:
    all_instruments: set[str] = set()
    for u_path in UNIVERSE_JSONS:
        data = json.loads(u_path.read_text("utf-8"))
        all_instruments.update(data["instruments"])
    return sorted(all_instruments)


def _write_bars(bars_by_iid: dict[str, list], gran: str, cat) -> None:
    from nautilus_trader.model.data import Bar, BarType
    from nautilus_trader.model.objects import Price, Quantity

    suffix = "1-MINUTE-LAST-EXTERNAL" if gran == "Minute" else "1-DAY-LAST-EXTERNAL"
    total = len(bars_by_iid)

    def _price(v: float) -> Price:
        return Price.from_str(f"{v:.1f}")

    for i, (iid, ticks) in enumerate(sorted(bars_by_iid.items()), 1):
        if not ticks:
            print(f"  [{i}/{total}] {iid} ({gran}): 0 rows — skip", flush=True)
            continue
        bar_type = BarType.from_str(f"{iid}-{suffix}")
        bars = [
            Bar(bar_type=bar_type, open=_price(o), high=_price(h), low=_price(l),
                close=_price(c), volume=Quantity.from_str(f"{int(v)}"), ts_event=ts, ts_init=ts)
            for ts, o, h, l, c, v in ticks
        ]
        cat.write_data(bars)
        print(f"  [{i}/{total}] {iid} ({gran}): wrote {len(bars)} bars", flush=True)


def build_minute(all_instruments: list[str], force: bool, cat) -> None:
    to_build = [iid for iid in all_instruments if force or _needs_build(iid, "Minute")]
    print(f"\n=== Minute bars: {len(to_build)}/{len(all_instruments)} to build ===")
    if not to_build:
        print("All minute bars already built.")
        return

    code_to_iid = {_instrument_to_code(iid): iid for iid in to_build}
    target_codes = set(code_to_iid)
    rows_by_iid: dict[str, list] = {iid: [] for iid in to_build}

    start_d = date.fromisoformat(MINUTE_START)
    end_d   = date.fromisoformat(MINUTE_END)

    for yyyymm in _iter_yyyymm(MINUTE_START, MINUTE_END):
        path = BASE_DIR / f"equities_bars_minute_{yyyymm}.csv.gz"
        if not path.exists():
            print(f"  [skip] {path.name} not found")
            continue
        t0 = time.time()
        n = 0
        print(f"  Reading {path.name} ...", end=" ", flush=True)
        with gzip.open(path, mode="rt", encoding="utf-8", newline="") as f:
            reader = csv.DictReader(f)
            for row in reader:
                code = row.get("Code", "")
                if code not in target_codes:
                    continue
                row_date = date.fromisoformat(row["Date"])
                if not (start_d <= row_date <= end_d):
                    continue
                h, m = map(int, row["Time"].split(":"))
                ts_ns = int(datetime.combine(row_date, dt_time(h, m, 59, 999999), tzinfo=_JST).timestamp() * 1e9)
                iid = code_to_iid[code]
                vol = float(row.get("Vo", 0) or 0)
                rows_by_iid[iid].append((ts_ns, float(row["O"]), float(row["H"]), float(row["L"]), float(row["C"]), vol))
                n += 1
        print(f"{n} rows in {time.time()-t0:.1f}s")

    _write_bars(rows_by_iid, "Minute", cat)


def build_daily(all_instruments: list[str], force: bool, cat) -> None:
    to_build = [iid for iid in all_instruments if force or _needs_build(iid, "Daily")]
    print(f"\n=== Daily bars: {len(to_build)}/{len(all_instruments)} to build ===")
    if not to_build:
        print("All daily bars already built.")
        return

    code_to_iid = {_instrument_to_code(iid): iid for iid in to_build}
    target_codes = set(code_to_iid)
    rows_by_iid: dict[str, list] = {iid: [] for iid in to_build}

    start_d = date.fromisoformat(DAILY_START)
    end_d   = date.fromisoformat(DAILY_END)

    for yyyymm in _iter_yyyymm(DAILY_START, DAILY_END):
        path = BASE_DIR / f"equities_bars_daily_{yyyymm}.csv.gz"
        if not path.exists():
            print(f"  [skip] {path.name} not found")
            continue
        t0 = time.time()
        n = 0
        print(f"  Reading {path.name} ...", end=" ", flush=True)
        with gzip.open(path, mode="rt", encoding="utf-8", newline="") as f:
            reader = csv.DictReader(f)
            for row in reader:
                code = row.get("Code", "")
                if code not in target_codes:
                    continue
                row_date = date.fromisoformat(row["Date"])
                if not (start_d <= row_date <= end_d):
                    continue
                ts_ns = int(datetime.combine(row_date, dt_time(15, 30), tzinfo=_JST).timestamp() * 1e9)
                iid = code_to_iid[code]
                try:
                    o, h, l, c = float(row["O"]), float(row["H"]), float(row["L"]), float(row["C"])
                except (ValueError, KeyError):
                    continue  # skip rows with empty/invalid OHLC
                vol = float(row.get("Vo", 0) or 0)
                rows_by_iid[iid].append((ts_ns, o, h, l, c, vol))
                n += 1
        print(f"{n} rows in {time.time()-t0:.1f}s")

    _write_bars(rows_by_iid, "Daily", cat)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--force", action="store_true", help="Rebuild even if already built")
    args = parser.parse_args()

    all_instruments = _load_all_instruments()
    print(f"Total unique instruments: {len(all_instruments)}")

    from nautilus_trader.persistence.catalog import ParquetDataCatalog
    CATALOG.mkdir(parents=True, exist_ok=True)
    cat = ParquetDataCatalog(str(CATALOG))

    build_minute(all_instruments, args.force, cat)
    build_daily(all_instruments, args.force, cat)
    print("\nDone.")


if __name__ == "__main__":
    main()
