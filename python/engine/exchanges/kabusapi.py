"""LiveVenueAdapter の kabuStation 実装。"""

from __future__ import annotations

import os
from typing import AsyncIterator, Literal

import asyncio

import httpx

from engine.exchanges import kabusapi_ws  # patch 対象を module 経由で参照
from engine.exchanges.kabusapi_register import RegisterSet
from engine.exchanges.kabusapi_url import endpoint
from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor
from engine.live.adapter import (
    Channel,
    DepthLevel,
    DepthUpdate,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    TradesUpdate,
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
        if self._client.is_closed:
            self._client = httpx.AsyncClient()
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
        if self._ws_task is not None:
            self._ws_task.cancel()
            try:
                await self._ws_task
            except asyncio.CancelledError:
                pass
            except BaseException:
                pass
        self._processors.clear()
        self._register_set.unregister_all()
        await self._client.aclose()
        self._token = None

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

    def _parse_instrument_id(self, instrument_id: InstrumentId) -> tuple[str, int]:
        symbol, _, suffix = instrument_id.rpartition(".")
        if suffix != "TSE":
            raise ValueError(f"unsupported exchange suffix: {suffix!r} (MVP supports TSE only)")
        return symbol, 1

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        if self._token is None:
            raise RuntimeError("login required before subscribe")
        symbol, exchange = self._parse_instrument_id(instrument_id)
        self._register_set.register(symbol, exchange)
        ok = await self._put_register(self._register_set.all_symbols())
        if not ok:
            self._register_set.unregister(symbol, exchange)
            raise RuntimeError(f"register failed: {symbol}")
        self._processors[symbol] = KabuPushFrameProcessor(symbol=symbol)
        if self._ws_task is None or self._ws_task.done():
            self._ws_task = asyncio.create_task(
                kabusapi_ws.connect(
                    env=self._env,
                    on_message=self._on_frame,
                    register_set=self._register_set,
                    put_register=self._put_register,
                )
            )

    async def _on_frame(self, msg: dict) -> None:
        symbol = msg.get("Symbol")
        if symbol is None:
            return
        proc = self._processors.get(symbol)
        if proc is None:
            return
        trade, depth = proc.process(msg)
        instrument_id = f"{symbol}.TSE"
        if depth is not None:
            self._queue.put_nowait(
                DepthUpdate(
                    kind="depth",
                    instrument_id=instrument_id,
                    ts_ns=depth["ts_ns"] or 0,
                    bids=tuple(DepthLevel(price=p, size=s) for p, s in depth["bids"]),
                    asks=tuple(DepthLevel(price=p, size=s) for p, s in depth["asks"]),
                )
            )
        if trade is not None:
            self._queue.put_nowait(
                TradesUpdate(
                    kind="trades",
                    instrument_id=instrument_id,
                    ts_ns=trade["ts_ns"] or 0,
                    price=trade["price"],
                    size=trade["size"],
                    aggressor_side=trade["aggressor_side"],
                )
            )

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        if self._token is None:
            return
        symbol, exchange = self._parse_instrument_id(instrument_id)
        self._register_set.unregister(symbol, exchange)
        self._processors.pop(symbol, None)
        await self._put_register(self._register_set.all_symbols())

    async def events(self) -> AsyncIterator[LiveEvent]:
        while True:
            yield await self._queue.get()
