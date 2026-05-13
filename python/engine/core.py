import logging
import time
import random
import threading
from typing import Optional, Literal
from .models import TradingState, EngineSnapshot, HistoryPoint
from .replay import BaseReplayProvider

class DataEngine:
    def __init__(self, replay_provider: Optional[BaseReplayProvider] = None, max_history_len: int = 1000):
        logging.info(f"Initializing DataEngine core (max_history_len: {max_history_len})")
        self._lock = threading.Lock()
        self._is_running = False
        self._replay_provider = replay_provider
        self._mode: Literal["static", "replay"] = "replay" if replay_provider else "static"
        self._is_exhausted = False
        self._max_history_len = max_history_len
        
        # 初期状態の設定
        if self._mode == "replay" and self._replay_provider:
            # Replay モードの場合は最初の 1 件でプライミングする
            tick = self._replay_provider.get_next_tick()
            if tick:
                self._timestamp, self._price = tick
                self._timestamp_ms = int(self._timestamp * 1000)
                self._history = [self._price]
                self._history_points = [HistoryPoint(timestamp_ms=self._timestamp_ms, price=self._price)]
                self._is_exhausted = self._replay_provider.is_exhausted()
                logging.info(f"Primed replay engine with first tick: {tick}")
            else:
                raise ValueError("Replay provider returned no data for priming")
        else:
            # Static モードのデフォルト (Phase 1/2 互換)
            self._price = 120.5
            self._history = [118.0, 119.0, 121.0, 120.5]
            self._timestamp = time.time()
            self._timestamp_ms = int(self._timestamp * 1000)
            # history 118, 119, 121, 120.5 に対してダミーの ms を割り当てる（簡略化のため 1s 刻み）
            self._history_points = [
                HistoryPoint(timestamp_ms=self._timestamp_ms - 3000, price=118.0),
                HistoryPoint(timestamp_ms=self._timestamp_ms - 2000, price=119.0),
                HistoryPoint(timestamp_ms=self._timestamp_ms - 1000, price=121.0),
                HistoryPoint(timestamp_ms=self._timestamp_ms, price=120.5),
            ]

    @property
    def is_running(self) -> bool:
        with self._lock:
            return self._is_running

    def start(self):
        with self._lock:
            logging.info(f"Starting DataEngine core (mode: {self._mode})")
            self._is_running = True

    def stop(self):
        with self._lock:
            logging.info("Stopping DataEngine core")
            self._is_running = False

    def advance(self):
        """内部状態を 1 ステップ進める (Thread-safe)"""
        with self._lock:
            if not self._is_running:
                return

            if self._replay_provider:
                tick = self._replay_provider.get_next_tick()
                if tick:
                    self._timestamp, self._price = tick
                    self._timestamp_ms = int(self._timestamp * 1000)
                    self._history.append(self._price)
                    self._history_points.append(HistoryPoint(timestamp_ms=self._timestamp_ms, price=self._price))
                    
                    if len(self._history) > self._max_history_len:
                        self._history.pop(0)
                        self._history_points.pop(0)
                        
                    self._is_exhausted = self._replay_provider.is_exhausted()
                else:
                    self._is_exhausted = True
                    logging.info("Replay data exhausted")
            else:
                self._price += random.uniform(-0.5, 0.5)
                self._timestamp = time.time()
                self._timestamp_ms = int(self._timestamp * 1000)
                self._history.append(self._price)
                self._history_points.append(HistoryPoint(timestamp_ms=self._timestamp_ms, price=self._price))
                
                if len(self._history) > self._max_history_len:
                    self._history.pop(0)
                    self._history_points.pop(0)

    def get_current_state(self) -> TradingState:
        """現在の最新状態を返す (Read-Only, Thread-safe)"""
        with self._lock:
            return TradingState(
                price=self._price,
                history=list(self._history), # コピーを渡す
                timestamp=self._timestamp,
                timestamp_ms=self._timestamp_ms,
                history_points=list(self._history_points)
            )

    def take_snapshot(self) -> EngineSnapshot:
        """現在のエンジンの実行コンテキストをスナップショットとして保存する (Thread-safe)"""
        with self._lock:
            source_path = None
            replay_index = 0
            if self._replay_provider and hasattr(self._replay_provider, 'file_path'):
                source_path = self._replay_provider.file_path
                replay_index = getattr(self._replay_provider, 'current_index', 0)

            return EngineSnapshot(
                state=TradingState(
                    price=self._price,
                    history=list(self._history),
                    timestamp=self._timestamp,
                    timestamp_ms=self._timestamp_ms,
                    history_points=list(self._history_points)
                ),
                replay_index=replay_index,
                source_path=source_path,
                mode=self._mode
            )

    def restore_snapshot(self, snapshot: EngineSnapshot):
        """スナップショットからエンジン状態を復元する (Thread-safe)"""
        with self._lock:
            # Mode mismatch check
            if snapshot.mode != self._mode:
                raise ValueError(f"Snapshot mode mismatch. Engine is {self._mode}, snapshot is {snapshot.mode}")

            # Source mismatch check (Replay mode only)
            if self._mode == "replay":
                if not self._replay_provider:
                    raise ValueError("Engine is in replay mode but has no provider to restore to")
                
                current_path = getattr(self._replay_provider, 'file_path', None)
                if snapshot.source_path and snapshot.source_path != current_path:
                    raise ValueError(f"Snapshot source mismatch. Expected {current_path}, got {snapshot.source_path}")

            self._price = snapshot.state.price
            self._history = list(snapshot.state.history)
            self._timestamp = snapshot.state.timestamp
            self._timestamp_ms = snapshot.state.timestamp_ms or int(self._timestamp * 1000)
            
            if snapshot.state.history_points:
                self._history_points = list(snapshot.state.history_points)
            else:
                # 過去のスナップショットとの互換性のため、history_points がない場合は history から生成（タイムスタンプは概算）
                count = len(self._history)
                self._history_points = [
                    HistoryPoint(timestamp_ms=self._timestamp_ms - (count - 1 - i) * 1000, price=p)
                    for i, p in enumerate(self._history)
                ]
            
            if self._replay_provider:
                if hasattr(self._replay_provider, 'current_index'):
                    self._replay_provider.current_index = snapshot.replay_index
                    self._is_exhausted = self._replay_provider.is_exhausted()
            
            logging.info(f"Restored snapshot (mode: {self._mode}, index: {snapshot.replay_index})")

    @property
    def is_exhausted(self) -> bool:
        with self._lock:
            return self._is_exhausted

    @property
    def mode(self) -> str:
        return self._mode
