"""LiveVenueAdapter の kabuStation 実装。"""

from __future__ import annotations

import os
from typing import AsyncIterator, Literal

import asyncio

import httpx

from engine.exchanges.kabusapi_register import RegisterSet
from engine.exchanges.kabusapi_url import endpoint
from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor
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
        self._client: httpx.AsyncClient = httpx.AsyncClient()
        self._register_set: RegisterSet = RegisterSet()
        self._processors: dict[str, KabuPushFrameProcessor] = {}
        self._queue: asyncio.Queue = asyncio.Queue()
        self._ws_task: asyncio.Task | None = None

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
        await self._client.aclose()

    async def _put_register(self, symbols: list[tuple[str, int]]) -> bool:
        """PUT /register で残存銘柄を再送する。

        Returns:
            ResultCode == 0 で True、それ以外 False。
        """
        resp = await self._client.put(
            endpoint("register", env=self._env),
            headers={"X-API-KEY": self._token},
            json={"Symbols": [{"Symbol": s, "Exchange": ex} for s, ex in symbols]},
        )
        data = resp.json()
        return data.get("ResultCode") == 0

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        return []

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        if self._token is None:
            raise RuntimeError("login required before subscribe")
        symbol, suffix = instrument_id.rsplit(".", 1)
        if suffix != "TSE":
            raise ValueError(f"unsupported exchange suffix: {suffix!r} (MVP supports TSE only)")
        self._register_set.register(symbol, 1)
        await self._put_register(self._register_set.all_symbols())
        self._processors[symbol] = KabuPushFrameProcessor(symbol=symbol)

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        if self._token is None:
            return
        symbol, _suffix = instrument_id.rsplit(".", 1)
        self._register_set.unregister(symbol, 1)
        self._processors.pop(symbol, None)
        await self._put_register(self._register_set.all_symbols())

    def events(self) -> AsyncIterator[LiveEvent]:
        raise NotImplementedError
