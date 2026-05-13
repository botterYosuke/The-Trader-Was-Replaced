import csv
import logging
import math
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

class SimpleCSVProvider(BaseReplayProvider):
    """最小構成の CSV (timestamp, price) を読み込む具体クラス"""
    
    def __init__(self, file_path: str):
        self.file_path = file_path
        self._data: List[Tuple[float, float]] = []
        self._index = 0
        self._load_csv()

    def _load_csv(self):
        """
        CSVを読み込み、内部バッファに格納する。
        読み込みに失敗した場合や有効なデータがない場合は例外を投げる。
        """
        try:
            with open(self.file_path, mode='r', encoding='utf-8') as f:
                reader = csv.reader(f)
                for row in reader:
                    if not row:
                        continue
                    try:
                        ts = float(row[0])
                        price = float(row[1])
                        
                        # Validation: Phase 2 の TradingState 契約に合わせる
                        if ts <= 0 or price <= 0 or not math.isfinite(ts) or not math.isfinite(price):
                            logging.debug(f"Skipping invalid data in CSV: {row}")
                            continue
                            
                        self._data.append((ts, price))
                    except (ValueError, IndexError):
                        logging.debug(f"Skipping non-numeric row in CSV: {row}")
            
            if not self._data:
                raise ValueError(f"No valid data found in CSV: {self.file_path}")
                
            logging.info(f"Loaded {len(self._data)} ticks from {self.file_path}")
        except FileNotFoundError:
            logging.error(f"CSV file not found: {self.file_path}")
            raise
        except Exception as e:
            logging.error(f"Failed to load CSV {self.file_path}: {e}")
            raise

    def get_next_tick(self) -> Optional[Tuple[float, float, float, float, float]]:
        if self._index < len(self._data):
            ts, price = self._data[self._index]
            self._index += 1
            return (ts, price, price, price, price)
        return None

    def is_exhausted(self) -> bool:
        return self._index >= len(self._data)

    def get_state_at(self, index: int) -> Optional[Tuple[float, float]]:
        """特定のインデックスのデータを取得する（スナップショット復元用）"""
        if 0 <= index < len(self._data):
            return self._data[index]
        return None

    @property
    def current_index(self) -> int:
        return self._index

    @current_index.setter
    def current_index(self, value: int):
        if 0 <= value <= len(self._data):
            self._index = value
        else:
            logging.warning(f"Invalid index for replay provider: {value}")


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
