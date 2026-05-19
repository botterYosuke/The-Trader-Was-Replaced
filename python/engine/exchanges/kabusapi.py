"""LiveVenueAdapter の kabuStation 実装。"""

from __future__ import annotations

import asyncio
import logging
import os
import time as _time_module
from typing import AsyncIterator, Awaitable, Callable, Literal, Optional

import httpx

from engine.exchanges import kabusapi_ws  # patch 対象を module 経由で参照
from engine.exchanges.kabusapi_auth import check_response
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

logger = logging.getLogger(__name__)

_ENV_API_PASSWORD = "DEV_KABU_API_PASSWORD"

# R5 rate-limit token bucket sizes (req/sec).
_INFO_RATE_PER_SEC = 10
_ORDER_RATE_PER_SEC = 5
_WALLET_RATE_PER_SEC = 10


class _TokenBucket:
    """Minimal async-friendly token bucket for R5 rate-limit pre-suppression.

    rate: tokens added per second (== capacity).
    Uses a *time_source* + injectable *sleep* so tests can drive it
    deterministically without sleeping real time.
    """

    def __init__(
        self,
        rate: int,
        *,
        time_source: Callable[[], float],
        sleep: Callable[[float], Awaitable[None]],
    ) -> None:
        self._rate = float(rate)
        self._capacity = float(rate)
        self._tokens = float(rate)
        self._last = time_source()
        self._time = time_source
        self._sleep = sleep
        self._lock = asyncio.Lock()

    async def acquire(self) -> None:
        async with self._lock:
            now = self._time()
            elapsed = now - self._last
            if elapsed > 0:
                self._tokens = min(
                    self._capacity, self._tokens + elapsed * self._rate
                )
                self._last = now
            if self._tokens < 1.0:
                await self._sleep((1.0 - self._tokens) / self._rate)
                self._tokens = 1.0
                self._last = self._time()
            self._tokens -= 1.0


class KabuStationAdapter:
    venue_id: str = "KABU"

    def __init__(
        self,
        environment: Literal["prod", "verify"] = "verify",
        *,
        time_source: Optional[Callable[[], float]] = None,
    ):
        if environment not in ("prod", "verify"):
            raise ValueError("environment must be 'prod' or 'verify'")
        self._env = environment
        self._token: str | None = None
        self._client: httpx.AsyncClient = httpx.AsyncClient()
        self._register_set: RegisterSet = RegisterSet()
        # Key by (Symbol, Exchange) per R4 — symbol alone collides across exchanges
        # (TSE=1, 名証=3, ...).
        self._processors: dict[tuple[str, int], KabuPushFrameProcessor] = {}
        self._queue: asyncio.Queue = asyncio.Queue()
        self._ws_task: asyncio.Task | None = None
        self._last_error: Optional[BaseException] = None
        # Per-symbol "warned once" set for ambiguous Exchange routing. Reset on
        # login()/logout() so a fresh session emits the warning again.
        self._exchange_ambiguity_warned: set[str] = set()
        # Rate-limit token buckets (R5). Tests inject _rate_limit_sleep.
        self._time_source: Callable[[], float] = time_source or _time_module.monotonic
        self._rate_limit_sleep: Callable[[float], Awaitable[None]] = asyncio.sleep
        self._info_bucket = _TokenBucket(
            _INFO_RATE_PER_SEC,
            time_source=self._time_source,
            sleep=lambda d: self._rate_limit_sleep(d),
        )

    @property
    def is_logged_in(self) -> bool:
        return self._token is not None

    @property
    def last_error(self) -> Optional[BaseException]:
        return self._last_error

    async def login(self, creds: VenueCredentials) -> None:
        # Clear _last_error only on the SUCCESS path (immediately before setting
        # _token). If a credential-validation raise happens first, callers keep
        # the prior error state instead of seeing a false "healthy" snapshot.
        if self._client.is_closed:
            self._client = httpx.AsyncClient()
        source = creds.credentials_source
        if source == "session_cache":
            raise ValueError("UNSUPPORTED_FOR_VENUE: kabu does not support session_cache")
        if source == "prompt_result":
            if not creds.token:
                raise ValueError("PROMPT_RESULT_MISSING_TOKEN")
            self._last_error = None
            self._exchange_ambiguity_warned.clear()
            self._token = creds.token
            return
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

        token = await fetch_token(api_password, env=self._env)
        self._last_error = None
        self._exchange_ambiguity_warned.clear()
        self._token = token

    async def logout(self) -> None:
        # Best-effort PUT /unregister/all (R6 cleanup). Tolerate any error —
        # token may already be invalid or kabu body may be down.
        if (
            self._token is not None
            and not self._client.is_closed
            and len(self._register_set) > 0
        ):
            try:
                await self._info_bucket.acquire()
                # 5s timeout — enough for localhost; body is best-effort cleanup.
                await self._client.put(
                    endpoint("unregister/all", env=self._env),
                    headers={"X-API-KEY": self._token},
                    timeout=httpx.Timeout(5.0),
                )
            except asyncio.CancelledError:
                raise
            except (
                httpx.HTTPError,
                asyncio.TimeoutError,
                RuntimeError,
                OSError,
            ) as exc:
                # OSError / ConnectionResetError can bubble up from a
                # closed/half-open transport during shutdown races.
                logger.warning("kabu unregister/all failed during logout: %s", exc)

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
        self._exchange_ambiguity_warned.clear()
        await self._client.aclose()
        self._token = None

    async def _put_register(self, symbols: list[tuple[str, int]]) -> bool:
        """PUT /register with R5 rate-limit + R7 two-stage error check.

        Raises:
            KabuApiError / KabuTokenExpiredError / KabuRegisterFullError /
            KabuRateLimitError on non-success responses (HIGH-1).

        Returns True on success (Code == 0).
        """
        await self._info_bucket.acquire()
        resp = await self._client.put(
            endpoint("register", env=self._env),
            headers={"X-API-KEY": self._token},
            json={"Symbols": [{"Symbol": s, "Exchange": ex} for s, ex in symbols]},
        )
        data = resp.json()
        # Some endpoints return ResultCode, others Code — normalize for check_response.
        if isinstance(data, dict) and "Code" not in data and "ResultCode" in data:
            data = {**data, "Code": data["ResultCode"]}
        check_response(data, resp.status_code)
        return True

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        return []

    def _parse_instrument_id(self, instrument_id: InstrumentId) -> tuple[str, int]:
        symbol, _, suffix = instrument_id.rpartition(".")
        if suffix != "TSE":
            raise ValueError(f"unsupported exchange suffix: {suffix!r} (MVP supports TSE only)")
        return symbol, 1

    async def _reset_all_processors(self) -> None:
        """HIGH-3: reset every processor's DV/quote state. Called on WS
        reconnect (codec docstring contract).
        """
        for proc in self._processors.values():
            proc.reset()

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        if self._token is None:
            raise RuntimeError("login required before subscribe")
        symbol, exchange = self._parse_instrument_id(instrument_id)
        was_registered = (symbol, exchange) in self._register_set
        self._register_set.register(symbol, exchange)
        try:
            await self._put_register(self._register_set.all_symbols())
        except BaseException:
            if not was_registered:
                self._register_set.unregister(symbol, exchange)
            raise
        if (symbol, exchange) not in self._processors:
            self._processors[(symbol, exchange)] = KabuPushFrameProcessor(symbol=symbol)
        if self._ws_task is None or self._ws_task.done():
            self._last_error = None
            self._ws_task = asyncio.create_task(self._run_ws())

    async def _run_ws(self) -> None:
        """Wrap kabusapi_ws.connect with last_error capture (MEDIUM-3)."""
        try:
            await kabusapi_ws.connect(
                env=self._env,
                on_message=self._on_frame,
                register_set=self._register_set,
                put_register=self._put_register,
                on_reconnect=self._reset_all_processors,
            )
        except asyncio.CancelledError:
            raise
        except BaseException as exc:
            self._last_error = exc
            raise

    async def _on_frame(self, msg: dict) -> None:
        symbol = msg.get("Symbol")
        if symbol is None:
            return
        # Round2 MEDIUM-2: key by (Symbol, Exchange). When the frame omits
        # Exchange, do NOT default to TSE=1 — silently mis-routing to the
        # wrong venue corrupts DV/quote state. Instead look up matching
        # processors and route only when unambiguous; otherwise drop with
        # a warning.
        exchange = msg.get("Exchange")
        if exchange is None:
            if symbol in self._exchange_ambiguity_warned:
                logger.debug(
                    "kabu frame for symbol %r missing Exchange; dropping (ambiguous routing)",
                    symbol,
                )
                return
            matches = [ex for (sym, ex) in self._processors.keys() if sym == symbol]
            if len(matches) == 1:
                exchange = matches[0]
            else:
                # Log once per symbol per session; subsequent drops are DEBUG to
                # avoid spam at kabu PUSH rates (hundreds of msg/sec).
                self._exchange_ambiguity_warned.add(symbol)
                logger.warning(
                    "kabu frame for symbol %r has no Exchange and matches "
                    "%d processors (%s); dropping (ambiguous routing). "
                    "Further occurrences for this symbol will log at DEBUG.",
                    symbol,
                    len(matches),
                    matches,
                )
                return
        proc = self._processors.get((symbol, exchange))
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
        if (symbol, exchange) not in self._register_set:
            return
        remaining = [s for s in self._register_set.all_symbols() if s != (symbol, exchange)]
        await self._put_register(remaining)
        self._register_set.unregister(symbol, exchange)
        self._processors.pop((symbol, exchange), None)

    async def events(self) -> AsyncIterator[LiveEvent]:
        while True:
            if self._queue.empty() and self._ws_task is not None and self._ws_task.done():
                exc = self._ws_task.exception()
                if exc is not None:
                    raise exc
                return
            get_task = asyncio.ensure_future(self._queue.get())
            try:
                if self._ws_task is None or self._ws_task.done():
                    yield await get_task
                    continue
                done, _pending = await asyncio.wait(
                    {get_task, self._ws_task},
                    return_when=asyncio.FIRST_COMPLETED,
                )
                if get_task in done:
                    yield get_task.result()
                else:
                    get_task.cancel()
                    exc = self._ws_task.exception()
                    if exc is not None:
                        raise exc
                    return
            except BaseException:
                if not get_task.done():
                    get_task.cancel()
                raise
