"""Phase 8 §1.3 LiveVenueAdapter の kabuStation 実装骨格。HTTP/WS は後続 step。"""

from __future__ import annotations

from typing import AsyncIterator, Literal

from engine.live.adapter import (
    Channel,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    LiveVenueAdapter,
    VenueCredentials,
)


class KabuStationAdapter:
    venue_id: str = "KABU"

    def __init__(self, environment: Literal["prod", "verify"] = "verify"):
        if environment not in ("prod", "verify"):
            raise ValueError("environment must be 'prod' or 'verify'")
        self._env = environment
        self._token: str | None = None

    async def login(self, creds: VenueCredentials) -> None:
        # kabu skill: session_cache は UNSUPPORTED_FOR_VENUE (skill ADR)
        if creds.credentials_source == "session_cache":
            raise ValueError("UNSUPPORTED_FOR_VENUE: kabu does not support session_cache")
        raise NotImplementedError("Phase 8 後半 HTTP client step で実装")

    async def logout(self) -> None:
        self._token = None

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        raise NotImplementedError("Phase 8 後半 HTTP client step で実装")

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        raise NotImplementedError("Phase 8 後半 WS step で実装")

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("Phase 8 後半 WS step で実装")

    def events(self) -> AsyncIterator[LiveEvent]:
        raise NotImplementedError("Phase 8 後半 WS step で実装")
