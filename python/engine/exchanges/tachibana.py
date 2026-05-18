"""Phase 8 §1.3 LiveVenueAdapter の Tachibana 実装骨格。HTTP/WS は後続 step。"""

from __future__ import annotations

from typing import AsyncIterator, Literal

from engine.live.adapter import (
    Channel,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    VenueCredentials,
)


class TachibanaAdapter:
    venue_id: str = "TACHIBANA"

    def __init__(self, environment: Literal["demo", "prod"] = "demo"):
        if environment not in ("demo", "prod"):
            raise ValueError("environment must be 'demo' or 'prod'")
        self._env = environment
        self._session: dict | None = None

    async def login(self, creds: VenueCredentials) -> None:
        raise NotImplementedError("Phase 8 後半 HTTP client step で実装")

    async def logout(self) -> None:
        self._session = None

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
