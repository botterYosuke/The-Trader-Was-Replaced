import logging
import time
import random
from typing import Optional
from .models import TradingState, EngineSnapshot
from .replay import BaseReplayProvider

class DataEngine:
    def __init__(self, replay_provider: Optional[BaseReplayProvider] = None):
        logging.info("Initializing DataEngine core")
        self.is_running = False
        self._price = 120.5
        self._history = [118.0, 119.0, 121.0, 120.5]
        self._timestamp = time.time()
        self._replay_provider = replay_provider
        self._mode = "replay" if replay_provider else "static"
        self._is_exhausted = False

    def start(self):
        logging.info(f"Starting DataEngine core (mode: {self._mode})")
        self.is_running = True

    def stop(self):
        logging.info("Stopping DataEngine core")
        self.is_running = False

    def advance(self):
        """
        内部状態を 1 ステップ進める。
        リプレイモードの場合は次のティックを読み込む。
        """
        if not self.is_running:
            return

        if self._replay_provider:
            tick = self._replay_provider.get_next_tick()
            if tick:
                self._timestamp, self._price = tick
                self._history.append(self._price)
                if len(self._history) > 1000:
                    self._history.pop(0)
                self._is_exhausted = self._replay_provider.is_exhausted()
            else:
                self._is_exhausted = True
                logging.info("Replay data exhausted")
        else:
            # スタティックモード（デモ用）: わずかに価格を変動させる
            self._price += random.uniform(-0.5, 0.5)
            self._timestamp = time.time()
            self._history.append(self._price)
            if len(self._history) > 1000:
                self._history.pop(0)

    def get_current_state(self) -> TradingState:
        """現在の最新状態を返す (Read-Only)"""
        return TradingState(
            price=self._price,
            history=self._history,
            timestamp=self._timestamp
        )

    def take_snapshot(self) -> EngineSnapshot:
        """現在のエンジンの実行コンテキストをスナップショットとして保存する"""
        source_path = None
        replay_index = 0
        if self._replay_provider and hasattr(self._replay_provider, 'file_path'):
            source_path = self._replay_provider.file_path
            replay_index = getattr(self._replay_provider, 'current_index', 0)

        return EngineSnapshot(
            state=self.get_current_state(),
            replay_index=replay_index,
            source_path=source_path,
            mode=self._mode
        )

    def restore_snapshot(self, snapshot: EngineSnapshot):
        """スナップショットからエンジン状態を復元する"""
        # Source mismatch check (Replay mode only)
        if self._mode == "replay" and self._replay_provider:
            current_path = getattr(self._replay_provider, 'file_path', None)
            if snapshot.source_path and snapshot.source_path != current_path:
                logging.warning(f"Snapshot source mismatch: snapshot={snapshot.source_path}, current={current_path}")
                # e-station 的にはエラーにするべきだが、Phase 3 では一旦警告に留めるか、
                # または厳密に弾く。ここでは厳密に弾くようにする。
                raise ValueError(f"Snapshot source mismatch. Expected {current_path}, got {snapshot.source_path}")

        self._price = snapshot.state.price
        self._history = list(snapshot.state.history)
        self._timestamp = snapshot.state.timestamp
        self._mode = snapshot.mode
        
        if self._replay_provider:
            if hasattr(self._replay_provider, 'current_index'):
                self._replay_provider.current_index = snapshot.replay_index
                # Exhausted status must be re-evaluated
                self._is_exhausted = self._replay_provider.is_exhausted()
        
        logging.info(f"Restored snapshot (mode: {self._mode}, index: {snapshot.replay_index})")

    @property
    def is_exhausted(self) -> bool:
        return self._is_exhausted

    @property
    def mode(self) -> str:
        return self._mode
