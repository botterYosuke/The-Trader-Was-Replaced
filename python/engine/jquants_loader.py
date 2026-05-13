import csv
import gzip
from datetime import date, datetime
from pathlib import Path
from zoneinfo import ZoneInfo

_JST = ZoneInfo("Asia/Tokyo")


def daily_rows_to_ticks(rows: list[dict[str, str]]) -> list[tuple[float, float]]:
    ticks = []
    for row in rows:
        ts = datetime(
            *date.fromisoformat(row["Date"]).timetuple()[:3], tzinfo=_JST
        ).timestamp()
        ticks.append((ts, float(row["C"])))
    return ticks


def instrument_id_to_jquants_code(instrument_id: str) -> str:
    symbol = instrument_id.split(".", 1)[0]
    if not symbol:
        raise ValueError(f"instrument_id has empty symbol: {instrument_id!r}")
    return f"{symbol}0"


_GRANULARITY_PREFIX = {
    "Trade": "equities_trades_",
    "Minute": "equities_bars_minute_",
    "Daily": "equities_bars_daily_",
}


class JQuantsLoader:
    def __init__(self, base_dir: str):
        self.base_dir = Path(base_dir)

    def check_data_exists(
        self,
        instrument_ids: list[str],
        start_date: str,
        end_date: str,
        granularity: str = "Trade",
    ) -> bool:
        if not self.base_dir.exists():
            return False

        if not instrument_ids:
            return False

        prefix = _GRANULARITY_PREFIX.get(granularity)
        if prefix is None:
            return False

        return any(
            (self.base_dir / f"{prefix}{yyyymm}.csv.gz").exists()
            for yyyymm in self._iter_yyyymm(start_date, end_date)
        )

    def load_daily_rows(
        self,
        instrument_id: str,
        start_date: str,
        end_date: str,
    ) -> list[dict[str, str]]:
        code = instrument_id_to_jquants_code(instrument_id)
        start = date.fromisoformat(start_date)
        end = date.fromisoformat(end_date)

        rows = []
        for yyyymm in self._iter_yyyymm(start_date, end_date):
            path = self.base_dir / f"equities_bars_daily_{yyyymm}.csv.gz"
            if not path.exists():
                continue

            with gzip.open(path, mode="rt", encoding="utf-8", newline="") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    row_date = date.fromisoformat(row["Date"])
                    if start <= row_date <= end and row["Code"] == code:
                        rows.append(row)

        return rows

    def _iter_yyyymm(self, start_date: str, end_date: str):
        start = date.fromisoformat(start_date)
        end = date.fromisoformat(end_date)

        if end < start:
            raise ValueError(
                f"end_date ({end_date}) must be >= start_date ({start_date})"
            )

        year, month = start.year, start.month
        while (year, month) <= (end.year, end.month):
            yield f"{year:04d}{month:02d}"
            if month == 12:
                year += 1
                month = 1
            else:
                month += 1
