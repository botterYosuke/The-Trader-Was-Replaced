from datetime import date
from pathlib import Path


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
