import logging
import random
import threading
import time
from typing import Literal, Optional

from .models import EngineSnapshot, HistoryPoint, TradingState
from .replay import BaseReplayProvider


class DataEngine:
    def __init__(
        self,
        replay_provider: Optional[BaseReplayProvider] = None,
        max_history_len: int = 1000,
    ):
        logging.info(
            f"Initializing DataEngine core (max_history_len: {max_history_len})"
        )
        self._lock = threading.Lock()
        self._is_running = False
        self._replay_state = "IDLE"
        self._replay_provider = replay_provider
        self._mode: Literal["static", "replay"] = (
            "replay" if replay_provider else "static"
        )
        self._is_exhausted = False
        self._max_history_len = max_history_len

        # Initialize the first visible state.
        if self._mode == "replay" and self._replay_provider:
            # Prime replay mode with the first tick so GetState has data immediately.
            tick = self._replay_provider.get_next_tick()
            if tick:
                self._timestamp, self._price = tick
                self._timestamp_ms = int(self._timestamp * 1000)
                self._history = [self._price]
                self._history_points = [
                    HistoryPoint(timestamp_ms=self._timestamp_ms, price=self._price)
                ]
                self._is_exhausted = self._replay_provider.is_exhausted()
                logging.info(f"Primed replay engine with first tick: {tick}")
            else:
                raise ValueError("Replay provider returned no data for priming")
        else:
            # Static mode fallback kept for Phase 1-5 compatibility.
            self._price = 120.5
            self._history = [118.0, 119.0, 121.0, 120.5]
            self._timestamp = time.time()
            self._timestamp_ms = int(self._timestamp * 1000)
            # Assign simple 1-second spaced timestamps for the static history.
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

    @property
    def replay_state(self) -> str:
        with self._lock:
            return self._replay_state

    def start(self):
        with self._lock:
            logging.info(f"Starting DataEngine core (mode: {self._mode})")
            self._is_running = True
            self._replay_state = "RUNNING"

    def stop(self):
        with self._lock:
            logging.info("Stopping DataEngine core")
            self._is_running = False
            self._replay_state = "IDLE"

    def load_replay_data(self) -> tuple[bool, str | None]:
        """
        Static mode uses legacy Start / Stop / GetState for Phase 1-5
        compatibility. Replay mode uses the Phase 6 replay controls.
        """
        with self._lock:
            if self._replay_state != "IDLE":
                return False, "LoadReplayData is only allowed from IDLE"

            if self._replay_provider is None:
                return False, "Replay provider is not configured"

            self._replay_state = "LOADED"
            return True, None

    def start_engine(self) -> tuple[bool, str | None]:
        with self._lock:
            if self._replay_state != "LOADED":
                return False, "StartEngine is only allowed from LOADED"

            self._is_running = True
            self._replay_state = "RUNNING"
            return True, None

    def pause_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            if self._replay_state != "RUNNING":
                return False, "PauseReplay is only allowed from RUNNING"

            self._is_running = False
            self._replay_state = "PAUSED"
            return True, None

    def resume_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            if self._replay_state != "PAUSED":
                return False, "ResumeReplay is only allowed from PAUSED"

            self._is_running = True
            self._replay_state = "RUNNING"
            return True, None

    def stop_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            if self._replay_state not in ("RUNNING", "PAUSED"):
                return False, "StopReplay is only allowed from RUNNING or PAUSED"

            self._is_running = False
            self._replay_state = "IDLE"
            return True, None

    def force_stop_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            self._is_running = False
            self._replay_state = "IDLE"
            return True, None

    def set_replay_speed(self, multiplier: int) -> tuple[bool, str | None]:
        with self._lock:
            if multiplier == 0:
                return False, "SetReplaySpeed multiplier must be greater than 0"

            return True, None

    def advance(self):
        """
        Advance one tick when the engine is running.

        The background advance loop calls this method. PAUSED replay sessions
        keep _is_running false, so they advance only through step_replay().
        """
        with self._lock:
            if not self._is_running:
                return

            self._advance_one_locked()

    def _advance_one_locked(self):
        """
        Advance exactly one tick.

        The _locked suffix means callers must already hold self._lock.
        """
        if self._replay_provider:
            tick = self._replay_provider.get_next_tick()
            if tick:
                self._timestamp, self._price = tick
                self._timestamp_ms = int(self._timestamp * 1000)
                self._history.append(self._price)
                self._history_points.append(
                    HistoryPoint(timestamp_ms=self._timestamp_ms, price=self._price)
                )

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
            self._history_points.append(
                HistoryPoint(timestamp_ms=self._timestamp_ms, price=self._price)
            )

            if len(self._history) > self._max_history_len:
                self._history.pop(0)
                self._history_points.pop(0)

    def step_replay(self) -> tuple[bool, str | None]:
        """Advance one tick while paused, then remain in PAUSED."""
        with self._lock:
            if self._replay_state != "PAUSED":
                return False, "StepReplay is only allowed from PAUSED"

            self._advance_one_locked()
            self._is_running = False
            self._replay_state = "PAUSED"
            return True, None

    def get_current_state(self) -> TradingState:
        """Return the current trading state as a read-only snapshot."""
        with self._lock:
            return TradingState(
                price=self._price,
                history=list(self._history),
                timestamp=self._timestamp,
                timestamp_ms=self._timestamp_ms,
                history_points=list(self._history_points),
            )

    def take_snapshot(self) -> EngineSnapshot:
        """Capture the current engine execution context."""
        with self._lock:
            source_path = None
            replay_index = 0
            if self._replay_provider and hasattr(self._replay_provider, "file_path"):
                source_path = self._replay_provider.file_path
                replay_index = getattr(self._replay_provider, "current_index", 0)

            return EngineSnapshot(
                state=TradingState(
                    price=self._price,
                    history=list(self._history),
                    timestamp=self._timestamp,
                    timestamp_ms=self._timestamp_ms,
                    history_points=list(self._history_points),
                ),
                replay_index=replay_index,
                source_path=source_path,
                mode=self._mode,
            )

    def restore_snapshot(self, snapshot: EngineSnapshot):
        """Restore engine state from a previously captured snapshot."""
        with self._lock:
            if snapshot.mode != self._mode:
                raise ValueError(
                    f"Snapshot mode mismatch. Engine is {self._mode}, snapshot is {snapshot.mode}"
                )

            if self._mode == "replay":
                if not self._replay_provider:
                    raise ValueError(
                        "Engine is in replay mode but has no provider to restore to"
                    )

                current_path = getattr(self._replay_provider, "file_path", None)
                if snapshot.source_path and snapshot.source_path != current_path:
                    raise ValueError(
                        f"Snapshot source mismatch. Expected {current_path}, got {snapshot.source_path}"
                    )

            self._price = snapshot.state.price
            self._history = list(snapshot.state.history)
            self._timestamp = snapshot.state.timestamp
            self._timestamp_ms = snapshot.state.timestamp_ms or int(
                self._timestamp * 1000
            )

            if snapshot.state.history_points:
                self._history_points = list(snapshot.state.history_points)
            else:
                # Older snapshots may not have history_points; reconstruct them.
                count = len(self._history)
                self._history_points = [
                    HistoryPoint(
                        timestamp_ms=self._timestamp_ms - (count - 1 - i) * 1000,
                        price=p,
                    )
                    for i, p in enumerate(self._history)
                ]

            if self._replay_provider:
                if hasattr(self._replay_provider, "current_index"):
                    self._replay_provider.current_index = snapshot.replay_index
                    self._is_exhausted = self._replay_provider.is_exhausted()

            logging.info(
                f"Restored snapshot (mode: {self._mode}, index: {snapshot.replay_index})"
            )

    @property
    def is_exhausted(self) -> bool:
        with self._lock:
            return self._is_exhausted

    @property
    def mode(self) -> str:
        return self._mode
