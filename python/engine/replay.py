import logging
from abc import ABC, abstractmethod
from typing import List, Optional, Tuple


class BaseReplayProvider(ABC):
    """リプレイデータの読み込みとイテレーションの抽象ベースクラス"""

    @abstractmethod
    def get_next_tick(self) -> Optional[Tuple[float, float, float, float, float]]:
        """
        次のティック (timestamp, open, high, low, close) を返す。
        データが終了した場合は None を返す。
        """
        pass

    @abstractmethod
    def is_exhausted(self) -> bool:
        """すべてのデータを読み終えたかどうかを返す"""
        pass


class NautilusBarsReplayProvider(BaseReplayProvider):
    """
    Replay provider backed by a ParquetDataCatalog.

    Eagerly loads Bars via the catalog loader, converts each to the 5-tuple shape
    (ts_sec, open, high, low, close) that DataEngine._prime_provider_locked /
    _advance_one_locked expect, and exposes them tick-by-tick.

    `bar_type` is the full BarType string used as the catalog `identifier`
    (e.g. "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL").
    """

    def __init__(
        self,
        catalog_path: str,
        bar_type: str,
        start=None,
        end=None,
    ):
        from .nautilus_catalog_loader import load_bars

        bars = load_bars(
            catalog_path,
            instrument_ids=[bar_type],
            start=start,
            end=end,
        )

        self._data: List[Tuple[float, float, float, float, float]] = [
            (
                int(bar.ts_event) / 1e9,
                bar.open.as_double(),
                bar.high.as_double(),
                bar.low.as_double(),
                bar.close.as_double(),
            )
            for bar in bars
        ]
        self._index = 0

        if not self._data:
            raise ValueError(
                f"No nautilus catalog bars found for {bar_type} at {catalog_path}"
            )

    def get_next_tick(self) -> Optional[Tuple[float, float, float, float, float]]:
        """Pop and return the next tick (alias for pop_next_tick)."""
        return self.pop_next_tick()

    def peek_next_tick(self) -> Optional[Tuple[float, float, float, float, float]]:
        """Return the next tick without advancing the index."""
        if self._index < len(self._data):
            return self._data[self._index]
        return None

    def pop_next_tick(self) -> Optional[Tuple[float, float, float, float, float]]:
        """Pop and return the next tick, advancing the index."""
        if self._index < len(self._data):
            tick = self._data[self._index]
            self._index += 1
            return tick
        return None

    def is_exhausted(self) -> bool:
        return self._index >= len(self._data)

    @property
    def current_index(self) -> int:
        return self._index

    @current_index.setter
    def current_index(self, value: int):
        if 0 <= value <= len(self._data):
            self._index = value
        else:
            logging.warning(f"Invalid index for NautilusBarsReplayProvider: {value}")
