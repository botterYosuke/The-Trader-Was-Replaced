from __future__ import annotations

_KNOWN_MODES = {"Replay", "LiveManual", "LiveAuto"}
_LIVE_OK_VENUE_STATES = {"CONNECTED", "SUBSCRIBED"}


class ModeManager:
    def __init__(self, venue_sm, replay_engine) -> None:
        self._venue_sm = venue_sm
        self._replay_engine = replay_engine
        self.current_mode: str = "Replay"

    def set_execution_mode(self, mode: str) -> str:
        if mode not in _KNOWN_MODES:
            raise ValueError(f"EXECUTION_MODE_PRECONDITION: unknown mode: {mode!r}")

        if mode in ("LiveManual", "LiveAuto"):
            venue_state = self._venue_sm.current
            if venue_state not in _LIVE_OK_VENUE_STATES:
                raise ValueError(
                    f"EXECUTION_MODE_PRECONDITION: venue must be CONNECTED/SUBSCRIBED, got {venue_state!r}"
                )
        self.current_mode = mode
        return mode
