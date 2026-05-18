"""Venue connection state machine for Phase 8 live trading."""

from __future__ import annotations


class InvalidVenueTransition(Exception):
    """Raised when an illegal state transition is requested."""


_STATES: frozenset[str] = frozenset(
    {
        "DISCONNECTED",
        "AUTHENTICATING",
        "CONNECTED",
        "SUBSCRIBED",
        "RECONNECTING",
        "ERROR",
    }
)

_ALLOWED: dict[str, set[str]] = {
    "DISCONNECTED": {"AUTHENTICATING"},
    "AUTHENTICATING": {"CONNECTED", "ERROR"},
    "CONNECTED": {"SUBSCRIBED", "ERROR"},
    "SUBSCRIBED": {"RECONNECTING", "ERROR"},
    "RECONNECTING": {"SUBSCRIBED", "ERROR"},
    "ERROR": set(),
}


class VenueStateMachine:
    def __init__(self) -> None:
        self.current: str = "DISCONNECTED"

    def transition_to(self, target: str) -> None:
        if target not in _STATES:
            raise InvalidVenueTransition(
                f"unknown target state: {target!r}"
            )
        if target not in _ALLOWED[self.current]:
            raise InvalidVenueTransition(
                f"illegal transition: {self.current} -> {target}"
            )
        self.current = target

    def reset(self) -> None:
        self.current = "DISCONNECTED"
