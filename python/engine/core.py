import logging
import random
import threading
import time
from typing import Literal, Optional

from .jquants_to_catalog import ensure_jquants_catalog
from .models import EngineSnapshot, HistoryPoint, OhlcPoint, TradingState
from .reducer import KlineUpdate, ReducerState, ReplayEvent, ReplayTimeUpdated, apply_event
from .replay import BaseReplayProvider, NautilusBarsReplayProvider


class DataEngine:
    def __init__(
        self,
        replay_provider: Optional[BaseReplayProvider] = None,
        max_history_len: int = 1000,
        jquants_loader=None,
        nautilus_catalog_path: Optional[str] = None,
        jquants_catalog_path: Optional[str] = None,
        state_machine: Optional["VenueStateMachine"] = None,
    ):
        logging.info(
            f"Initializing DataEngine core (max_history_len: {max_history_len})"
        )
        self.state_machine = state_machine
        self._lock = threading.Lock()
        self._is_running = False
        self._replay_state = "IDLE"
        # Gate for engine_runner: SET = running, CLEAR = paused.
        self._run_event = threading.Event()
        self._run_event.set()
        self._replay_provider = replay_provider
        self._mode: Literal["static", "replay"] = (
            "replay" if replay_provider else "static"
        )
        self._is_exhausted = False
        self._max_history_len = max_history_len
        self._jquants_loader = jquants_loader
        self._nautilus_catalog_path = nautilus_catalog_path
        self._jquants_catalog_path = jquants_catalog_path
        self._event_log: list[ReplayEvent] = []
        self._last_replay_catalog_path: Optional[str] = None
        self.last_portfolio: Optional[dict] = None

        # Initialize the first visible state.
        if self._mode == "replay" and self._replay_provider:
            self._prime_provider_locked(self._replay_provider)
        else:
            # Static mode fallback kept for Phase 1-5 compatibility.
            ts_ms = int(time.time() * 1000)
            self._rs = ReducerState(
                timestamp_ms=ts_ms,
                price=120.5,
                history=[118.0, 119.0, 121.0, 120.5],
                history_points=[
                    HistoryPoint(timestamp_ms=ts_ms - 3000, price=118.0),
                    HistoryPoint(timestamp_ms=ts_ms - 2000, price=119.0),
                    HistoryPoint(timestamp_ms=ts_ms - 1000, price=121.0),
                    HistoryPoint(timestamp_ms=ts_ms, price=120.5),
                ],
                max_history_len=max_history_len,
            )

    @property
    def is_running(self) -> bool:
        with self._lock:
            return self._is_running

    @property
    def replay_state(self) -> str:
        with self._lock:
            return self._replay_state

    def apply_replay_event(self, event: ReplayEvent) -> None:
        with self._lock:
            self._apply_event_locked(event)

    def _apply_event_locked(self, event: ReplayEvent) -> None:
        self._event_log.append(event)
        apply_event(self._rs, event)

    def _prime_provider_locked(self, provider: BaseReplayProvider) -> None:
        tick = provider.get_next_tick()
        if not tick:
            raise ValueError("Replay provider returned no data for priming")
        self._replay_provider = provider
        self._mode = "replay"
        ts, o, h, l, c = tick
        ts_ms = int(ts * 1000)
        self._rs = ReducerState(
            timestamp_ms=ts_ms,
            price=c,
            open=o,
            high=h,
            low=l,
            history=[c],
            history_points=[HistoryPoint(timestamp_ms=ts_ms, price=c)],
            ohlc_points=[OhlcPoint(timestamp_ms=ts_ms, open_time_ms=ts_ms, open=o, high=h, low=l, close=c)],
            max_history_len=self._max_history_len,
        )
        self._is_exhausted = provider.is_exhausted()
        logging.info(f"Primed replay engine with first tick: {tick}")

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

    def load_replay_data(
        self,
        instrument_ids: list[str] | None = None,
        start_date: str = "",
        end_date: str = "",
        granularity: str = "Trade",
        catalog_path: str | None = None,
    ) -> tuple[bool, str | None]:
        """
        Static mode uses legacy Start / Stop / GetState for Phase 1-5
        compatibility. Replay mode uses the Phase 6 replay controls.
        """
        with self._lock:
            if self._replay_state != "IDLE":
                return False, "LoadReplayData is only allowed from IDLE"

            if self._replay_provider is not None:
                self._replay_state = "LOADED"
                return True, None

            instrument_ids = instrument_ids or []
            if not instrument_ids:
                return False, "At least one instrument_id is required"

            effective_catalog_path = catalog_path or self._nautilus_catalog_path
            if effective_catalog_path is not None:
                if granularity not in ("Daily", "Minute"):
                    return False, f"Unsupported granularity for nautilus catalog: {granularity!r}"

                try:
                    provider = NautilusBarsReplayProvider(
                        catalog_path=effective_catalog_path,
                        bar_type=instrument_ids[0],
                        start=start_date or None,
                        end=end_date or None,
                    )
                except (ValueError, FileNotFoundError) as e:
                    return False, str(e)

                self._prime_provider_locked(provider)
                self._last_replay_catalog_path = effective_catalog_path
                self._replay_state = "LOADED"
                return True, None

            if self._jquants_loader is not None:
                if granularity not in ("Daily", "Minute"):
                    return False, f"Unsupported granularity for replay: {granularity!r}"

                if not self._jquants_catalog_path:
                    return False, "J-Quants catalog path is not configured"

                try:
                    result = ensure_jquants_catalog(
                        base_dir=self._jquants_loader.base_dir,
                        catalog_path=self._jquants_catalog_path,
                        instrument_id=instrument_ids[0],
                        start_date=start_date,
                        end_date=end_date,
                        granularity=granularity,
                    )
                    provider = NautilusBarsReplayProvider(
                        catalog_path=result.catalog_path,
                        bar_type=result.bar_type,
                        start=None,
                        end=None,
                    )
                except (ValueError, FileNotFoundError) as e:
                    return False, str(e)

                self._prime_provider_locked(provider)
                self._last_replay_catalog_path = result.catalog_path
                self._replay_state = "LOADED"
                return True, None

            return False, "Replay provider is not configured"

    @property
    def last_replay_catalog_path(self) -> str | None:
        return self._last_replay_catalog_path

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
            self._run_event.clear()
            return True, None

    def resume_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            if self._replay_state != "PAUSED":
                return False, "ResumeReplay is only allowed from PAUSED"

            self._is_running = True
            self._replay_state = "RUNNING"
            self._run_event.set()
            return True, None

    def stop_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            if self._replay_state not in ("RUNNING", "PAUSED"):
                return False, "StopReplay is only allowed from RUNNING or PAUSED"

            self._is_running = False
            self._replay_state = "IDLE"
            self._run_event.set()
            return True, None

    def force_stop_replay(self) -> tuple[bool, str | None]:
        with self._lock:
            self._is_running = False
            self._replay_state = "IDLE"
            self._run_event.set()
            return True, None

    @property
    def run_event(self) -> threading.Event:
        return self._run_event

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
                ts, o, h, l, c = tick
                ts_ms = int(ts * 1000)
                self._apply_event_locked(ReplayTimeUpdated(timestamp_ms=ts_ms))
                self._apply_event_locked(KlineUpdate(timestamp_ms=ts_ms, close=c, open=o, high=h, low=l, open_time_ms=ts_ms))
                self._is_exhausted = self._replay_provider.is_exhausted()
            else:
                self._is_exhausted = True
                logging.info("Replay data exhausted")
        else:
            price = self._rs.price + random.uniform(-0.5, 0.5)
            ts_ms = int(time.time() * 1000)
            self._apply_event_locked(KlineUpdate(timestamp_ms=ts_ms, close=price, open=price, high=price, low=price))

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
            rs = self._rs
            return TradingState(
                price=rs.price,
                history=list(rs.history),
                timestamp=rs.timestamp_ms / 1000.0,
                timestamp_ms=rs.timestamp_ms,
                history_points=list(rs.history_points),
                ohlc_points=list(rs.ohlc_points),
                open=rs.open or None,
                high=rs.high or None,
                low=rs.low or None,
                close=rs.price,
                open_time_ms=rs.open_time_ms or None,
                replay_state=self._replay_state,
            )

    def take_snapshot(self) -> EngineSnapshot:
        """Capture the current engine execution context."""
        with self._lock:
            source_path = None
            replay_index = 0
            if self._replay_provider:
                if hasattr(self._replay_provider, "file_path"):
                    source_path = self._replay_provider.file_path
                if hasattr(self._replay_provider, "current_index"):
                    replay_index = self._replay_provider.current_index

            rs = self._rs
            return EngineSnapshot(
                state=TradingState(
                    price=rs.price,
                    history=list(rs.history),
                    timestamp=rs.timestamp_ms / 1000.0,
                    timestamp_ms=rs.timestamp_ms,
                    history_points=list(rs.history_points),
                    ohlc_points=list(rs.ohlc_points),
                    open=rs.open or None,
                    high=rs.high or None,
                    low=rs.low or None,
                    close=rs.price,
                    open_time_ms=rs.open_time_ms or None,
                    replay_state=self._replay_state,
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

            ts_ms = snapshot.state.timestamp_ms or int(snapshot.state.timestamp * 1000)

            if snapshot.state.history_points:
                history_points = list(snapshot.state.history_points)
            else:
                # Older snapshots may not have history_points; reconstruct them.
                count = len(snapshot.state.history)
                history_points = [
                    HistoryPoint(
                        timestamp_ms=ts_ms - (count - 1 - i) * 1000,
                        price=p,
                    )
                    for i, p in enumerate(snapshot.state.history)
                ]

            self._rs.price = snapshot.state.price
            self._rs.timestamp_ms = ts_ms
            self._rs.history = list(snapshot.state.history)
            self._rs.history_points = history_points
            self._rs.ohlc_points = list(snapshot.state.ohlc_points)
            self._rs.open = snapshot.state.open or snapshot.state.price
            self._rs.high = snapshot.state.high or snapshot.state.price
            self._rs.low = snapshot.state.low or snapshot.state.price
            self._rs.open_time_ms = snapshot.state.open_time_ms or ts_ms

            if self._replay_provider:
                if hasattr(self._replay_provider, "current_index"):
                    self._replay_provider.current_index = snapshot.replay_index
                    self._is_exhausted = self._replay_provider.is_exhausted()

            logging.info(
                f"Restored snapshot (mode: {self._mode}, index: {snapshot.replay_index})"
            )

    def get_event_log(self) -> list[ReplayEvent]:
        with self._lock:
            return list(self._event_log)

    @property
    def is_exhausted(self) -> bool:
        with self._lock:
            return self._is_exhausted

    @property
    def mode(self) -> str:
        return self._mode
