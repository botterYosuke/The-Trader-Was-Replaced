"""LiveVenueAdapter の kabuStation 実装。"""

from __future__ import annotations

import os
from typing import AsyncIterator, Literal

from engine.live.adapter import (
    Channel,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    VenueCredentials,
)

_ENV_API_PASSWORD = "DEV_KABU_API_PASSWORD"


class KabuStationAdapter:
    venue_id: str = "KABU"

    def __init__(self, environment: Literal["prod", "verify"] = "verify"):
        if environment not in ("prod", "verify"):
            raise ValueError("environment must be 'prod' or 'verify'")
        self._env = environment
        self._token: str | None = None

    async def login(self, creds: VenueCredentials) -> None:
        source = creds.credentials_source
        if source == "session_cache":
            raise ValueError("UNSUPPORTED_FOR_VENUE: kabu does not support session_cache")
        if source == "prompt":
            raise NotImplementedError("prompt credentials_source not yet supported for kabu")
        if source != "env":
            raise ValueError(f"unknown credentials_source: {source!r}")

        api_password = os.environ.get(_ENV_API_PASSWORD)
        if not api_password:
            raise ValueError(
                f"missing env credentials: {_ENV_API_PASSWORD} "
                f"(credentials_source='env')"
            )

        from engine.exchanges.kabusapi_auth import fetch_token

        self._token = await fetch_token(api_password, env=self._env)

    async def logout(self) -> None:
        self._token = None

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        return []

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        raise NotImplementedError

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError

    def events(self) -> AsyncIterator[LiveEvent]:
        raise NotImplementedError
