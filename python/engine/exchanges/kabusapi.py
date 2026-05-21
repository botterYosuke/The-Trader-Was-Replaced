"""LiveVenueAdapter の kabuStation 実装。"""

from __future__ import annotations

import asyncio
import logging
import os
import time as _time_module
import uuid
from dataclasses import dataclass
from typing import AsyncIterator, Awaitable, Callable, Literal, Optional

import httpx

from engine.exchanges import kabusapi_orders as _orders
from engine.exchanges import kabusapi_ws  # patch 対象を module 経由で参照
from engine.exchanges.kabusapi_auth import KabuApiError, auth_headers, check_response
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
from engine.live.order_types import (
    AccountPositionData,
    AccountSnapshot,
    OrderEventData,
    OrderResult,
)

logger = logging.getLogger(__name__)

_ENV_API_PASSWORD = "DEV_KABU_API_PASSWORD"

# R5 rate-limit token bucket sizes (req/sec).
_INFO_RATE_PER_SEC = 10
_ORDER_RATE_PER_SEC = 5
_WALLET_RATE_PER_SEC = 10

# kabuステーション本体がログアウト / 未ログインのときに REST が返す Code (R7、
# ptal/error.html)。`4001007`=ログイン認証エラー / `4001017`=ログイン認証エラー
# (本体未ログイン)。Watchdog (Phase 9 §3.5 / Step 7) が check_health() でこれを検出し、
# 本体早朝強制ログアウト → 再ログイン誘導の起点にする。
_VENUE_LOGGED_OUT_CODES = frozenset({4001007, 4001017})

# 約定確認 polling 間隔 (§3.3.2: GET /orders を 1 秒間隔)。
_ORDERS_POLL_INTERVAL_S = 1.0
# polling が連続失敗 (本体ログアウト / 401 / 接続断) したときの最大バックオフ秒。
# 1Hz hot-loop と R5 流量浪費を避けるため、失敗ごとに指数的に延ばす上限。
_POLL_MAX_BACKOFF_S = 30.0
# 訂正 (取消→新規) で取消確定を待つ最大ポーリング回数 (×_ORDERS_POLL_INTERVAL_S)。
_MODIFY_CANCEL_WAIT_POLLS = 10
# 発注/取消/口座系 REST のタイムアウト (localhost なので短くて十分)。
_ORDER_TIMEOUT = httpx.Timeout(connect=10.0, read=30.0, write=10.0, pool=5.0)


# on_order_event(OrderEventData) -> None : GET /orders polling で検出した注文状態変化を
# proto 化して push する transport コールバック (server_grpc が publish_backend_event に
# 束ねて注入)。kabu は約定 PUSH を持たないため polling 由来 (§3.3.2)。
OnOrderEvent = Callable[[OrderEventData], None]


@dataclass
class _KabuOrderRef:
    """発注済み注文の追跡情報 (取消/訂正/polling・訂正の再発注で使う)。

    kabu の訂正は「取消 → 新規発注」変換 (§2.2) で venue OrderId が更新されるため、
    本 ref は **mutable** にして同一 client_order_id に新 OrderId を再マップする。
    再発注に必要な元注文パラメータ (symbol/side/qty/price/...) を保持する。

    ``filled_base`` / ``notional_base`` は **訂正で捨てた旧 venue leg の累計約定**を
    退避する。in-place remap は旧 OrderId を ``_order_id_to_cid`` から外す (= polling が
    旧 leg を二度と読まない) ため、これを退避しないと取消確定までに約定していた数量が
    OrderEvent stream から永久に消えて filled_qty が過少報告される (review HIGH)。
    polling は ``filled_base + 新 leg の CumQty`` を論理注文の累計として push する。
    ``qty`` は現在の venue leg の発注数量 (= 残数量) で、論理注文の総目標数量は
    常に ``filled_base + qty`` で表される。
    """

    client_order_id: str
    order_id: str  # venue 採番の OrderId (訂正で更新される)
    symbol: str
    exchange: int
    side: str
    qty: float
    price: float | None
    order_type: str
    time_in_force: str
    account_type: int
    # 訂正で捨てた旧 leg の累計約定数量 / 約定代金 (加重平均価格の算出に使う)。
    filled_base: float = 0.0
    notional_base: float = 0.0


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
        self._order_bucket = _TokenBucket(
            _ORDER_RATE_PER_SEC,
            time_source=self._time_source,
            sleep=lambda d: self._rate_limit_sleep(d),
        )
        self._wallet_bucket = _TokenBucket(
            _WALLET_RATE_PER_SEC,
            time_source=self._time_source,
            sleep=lambda d: self._rate_limit_sleep(d),
        )
        # Phase 9 Step 6: 発注経路 (set_execution_hooks で注入)。kabu は Password 不要
        # (R3) のため secret_resolver は使わない。約定通知は GET /orders polling (§3.3.2)。
        self._on_order_event: OnOrderEvent | None = None
        self._orders_ref: dict[str, _KabuOrderRef] = {}  # client_order_id -> ref
        self._order_id_to_cid: dict[str, str] = {}  # venue OrderId -> client_order_id
        # client_order_id -> 直近 push 済み (status, filled_qty)。polling の重複 push 抑止。
        self._last_pushed: dict[str, tuple[str, float]] = {}
        # 訂正 (取消→新規) 進行中の client_order_id。polling が中間状態 (取消確定) を
        # spurious な CANCELED として push / unregister するのを抑止する。
        self._modifying: set[str] = set()
        self._orders_poll_task: asyncio.Task | None = None

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
        # Re-login without an intervening logout(): tear down the prior orders
        # poll loop and order registry so stale notifications / id mappings don't
        # bleed across sessions (Tachibana adapter と同方針)。
        await self._stop_orders_poll()
        self._orders_ref.clear()
        self._order_id_to_cid.clear()
        self._last_pushed.clear()
        self._modifying.clear()
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
            except Exception as exc:
                # 想定は CancelledError のみ。WS task のシャットダウン時バグは握り潰さず
                # ログに残す (silent failure 回避)。
                logger.warning("kabu WS task errored during logout: %s", exc)
        await self._stop_orders_poll()
        self._orders_ref.clear()
        self._order_id_to_cid.clear()
        self._last_pushed.clear()
        self._modifying.clear()
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

    # ------------------------------------------------------------------
    # Phase 9 Step 6: OrderingVenueAdapter — 発注 / 取消 / 訂正 / 口座
    # kabu は Password 不要 (R3)・約定 PUSH 無し → GET /orders polling で約定通知 (§3.3.2)。
    # ------------------------------------------------------------------

    def set_execution_hooks(
        self,
        *,
        secret_resolver: object = None,
        on_order_event: OnOrderEvent,
        on_venue_logout: object = None,
    ) -> None:
        """server_grpc が OrderEvent push を注入する。

        Tachibana と同じ呼び出し口だが、kabu は Password 不要 (R3) のため
        ``secret_resolver`` は受理して無視する。約定通知は GET /orders polling 由来
        なので、polling は最初の ``submit_order`` で遅延起動する (idle polling 回避)。
        ``on_venue_logout`` も受理して無視する: kabu の本体ログアウト検知は push ではなく
        poll 型の VenueHealthWatchdog (check_health → GET /apisoftlimit) で行う (§3.5)。
        """
        self._on_order_event = on_order_event

    @staticmethod
    def _rejected_result(
        client_order_id: str, ack: "_orders.SendOrderAck"
    ) -> OrderResult:
        """発注エラー (Result != 0) を REJECTED な OrderResult に正規化する。"""
        return OrderResult(
            status="REJECTED",
            filled_qty=0.0,
            avg_price=None,
            client_order_id=client_order_id,
            reject_reason=f"{ack.reject_code}:{ack.reject_text}",
        )

    def _register_order(self, ref: _KabuOrderRef) -> None:
        self._orders_ref[ref.client_order_id] = ref
        self._order_id_to_cid[ref.order_id] = ref.client_order_id

    def _unregister_order(self, client_order_id: str) -> None:
        ref = self._orders_ref.pop(client_order_id, None)
        if ref is not None:
            self._order_id_to_cid.pop(ref.order_id, None)
        self._last_pushed.pop(client_order_id, None)

    async def _send_order(self, payload: dict[str, object]) -> "_orders.SendOrderAck":
        """POST /sendorder を流量抑制付きで叩き、HTTP/Code + Result を判定して ack を返す。

        ``_cancel_venue_order`` と対称 (どちらも SendOrderAck を返す)。
        """
        await self._order_bucket.acquire()
        resp = await self._client.post(
            endpoint("sendorder", env=self._env),
            headers=auth_headers(self._token or ""),
            json=payload,
            timeout=_ORDER_TIMEOUT,
        )
        data = resp.json()
        check_response(data, resp.status_code)
        return _orders.parse_send_order_response(data)

    async def _cancel_venue_order(self, order_id: str) -> "_orders.SendOrderAck":
        """PUT /cancelorder を流量抑制付きで叩く (OrderID のみ・Password 不要、R3)。"""
        await self._order_bucket.acquire()
        resp = await self._client.put(
            endpoint("cancelorder", env=self._env),
            headers=auth_headers(self._token or ""),
            json=_orders.build_cancel_order_payload(order_id=order_id),
            timeout=_ORDER_TIMEOUT,
        )
        data = resp.json()
        check_response(data, resp.status_code)
        return _orders.parse_send_order_response(data)

    async def submit_order(
        self,
        *,
        venue: str,
        instrument_id: InstrumentId,
        side: str,
        qty: float,
        price: float | None,
        order_type: str,
        time_in_force: str,
        **extra: object,
    ) -> OrderResult:
        """POST /sendorder で現物新規発注する (Password 不要)。

        受付成立で ACCEPTED を返す。約定 (FILLED/PARTIALLY_FILLED) は GET /orders polling
        で後追いし OrderEvent として push する。発注エラー Result != 0 は REJECTED に正規化
        (ただし Result == -1 の異常終了は KabuApiError を上層へ伝播、§2.2)。
        """
        if self._token is None:
            raise RuntimeError("submit_order requires login; call login() first")
        symbol, exchange = self._parse_instrument_id(instrument_id)
        payload = _orders.build_send_order_payload(
            symbol=symbol,
            exchange=exchange,
            side=side,
            qty=qty,
            price=price,
            order_type=order_type,
            time_in_force=time_in_force,
        )
        ack = await self._send_order(payload)
        client_order_id = uuid.uuid4().hex
        if ack.rejected:
            if ack.reject_code == "-1":
                # 異常終了コード (システムエラー): トーストで明示するため上層へ伝播 (§2.2)。
                raise KabuApiError(-1, ack.reject_text or "kabu sendorder system error")
            return self._rejected_result(client_order_id, ack)
        if not ack.order_id:
            # 受付 (Result==0) なのにサーバが OrderId を採番していない応答は追跡不能
            # (R9: OrderID はサーバ採番)。空文字を _order_id_to_cid に入れると ID 空の
            # 任意 /orders 行と誤マッチする latent cross-match になるため、登録せず
            # KabuApiError を上層へ伝播する (fix #3)。
            raise KabuApiError(
                0, "kabu sendorder accepted but returned no OrderId"
            )
        self._register_order(
            _KabuOrderRef(
                client_order_id=client_order_id,
                order_id=ack.order_id,
                symbol=symbol,
                exchange=exchange,
                side=side.upper(),
                qty=qty,
                price=price,
                order_type=order_type.upper(),
                time_in_force=time_in_force,
                account_type=_orders.DEFAULT_ACCOUNT_TYPE,
            )
        )
        self._ensure_orders_poll()
        return OrderResult(
            status="ACCEPTED", filled_qty=0.0, avg_price=None,
            client_order_id=client_order_id,
        )

    async def cancel_order(
        self, *, venue: str, order_id: str, **extra: object
    ) -> OrderResult:
        """PUT /cancelorder で取消する (OrderID のみ・Password 不要)。

        取消受付成立で CANCELED を返す (確定状態は polling が後追い)。Result != 0 の
        取消拒否は REJECTED (facade が CANCEL_REJECTED に変換し元注文は live のまま)。
        """
        if self._token is None:
            raise RuntimeError("cancel_order requires login; call login() first")
        if order_id in self._modifying:
            # 訂正 (取消→新規) 進行中の注文を並行取消すると、modify が remap した新 leg を
            # 孤児化させうる (cancel↔modify re-entrancy)。modify 完了まで弾く。
            return OrderResult(
                status="REJECTED", filled_qty=0.0, avg_price=None,
                client_order_id=order_id, reject_reason="MODIFY_IN_PROGRESS",
            )
        ref = self._orders_ref.get(order_id)
        if ref is None:
            return OrderResult(
                status="REJECTED", filled_qty=0.0, avg_price=None,
                client_order_id=order_id, reject_reason="UNKNOWN_VENUE_ORDER",
            )
        ack = await self._cancel_venue_order(ref.order_id)
        if ack.rejected:
            return self._rejected_result(order_id, ack)
        return OrderResult(
            status="CANCELED", filled_qty=0.0, avg_price=None, client_order_id=order_id,
        )

    async def modify_order(
        self,
        *,
        venue: str,
        order_id: str,
        new_price: float | None = None,
        new_qty: float | None = None,
        **extra: object,
    ) -> OrderResult:
        """訂正を「取消 → 新規発注」変換で実現する (kabu に訂正 API は無い、§2.2)。

        atomicity は保証されない。補償結果を facade 契約に合わせて OrderResult.status で
        表現する (proto 非変更の Step 5 方針を踏襲):

        - 取消失敗 → ``REJECTED`` (facade が MODIFY_REJECTED。**元注文は live のまま**)。
        - 取消成功 + 新規失敗 → ``CANCELED`` (facade が同一注文を CANCELED 終端化。
          **元注文は取消済み**で新規は出ていない → UI は取消済みとして正しく表示。
          ユーザーは再発注すればよい)。
        - 取消確定待ちタイムアウト → ``REJECTED`` (新規は見送り。元注文の確定状態は
          polling が後追いで反映する)。
        - 全成功 → ``ACCEPTED`` (同一 client_order_id に新 OrderId を再マップ。polling は
          新 OrderId を同じ注文として追跡する)。
        """
        if self._token is None:
            raise RuntimeError("modify_order requires login; call login() first")
        if order_id in self._modifying:
            # 同一注文への多重訂正を弾く (cancel_order の MODIFY_IN_PROGRESS と対称)。
            # 先行 modify が remap 中に二本目が走ると、二本目の finally が _modifying を
            # 先に畳んで suppression window を壊し、polling が先行 leg の中間状態を
            # spurious に push / unregister しうる。先行完了まで待たせる。
            return OrderResult(
                status="REJECTED", filled_qty=0.0, avg_price=None,
                client_order_id=order_id, reject_reason="MODIFY_IN_PROGRESS",
            )
        ref = self._orders_ref.get(order_id)
        if ref is None:
            return OrderResult(
                status="REJECTED", filled_qty=0.0, avg_price=None,
                client_order_id=order_id, reject_reason="UNKNOWN_VENUE_ORDER",
            )

        # 取消→新規の途中で background polling が「取消確定」を spurious な CANCELED
        # として push / unregister しないよう、この注文の polling を一時抑止する。
        self._modifying.add(order_id)
        try:
            # 1. 元注文を取消す。
            cancel_ack = await self._cancel_venue_order(ref.order_id)
            if cancel_ack.rejected:
                return OrderResult(
                    status="REJECTED", filled_qty=0.0, avg_price=None,
                    client_order_id=order_id,
                    reject_reason="MODIFY_CANCEL_FAILED:原注文は残っています",
                )

            # 2. 取消確定 (State==5) を待ち、確定時点の OrderStatusReport を得る。確認でき
            #    なければ新規は見送る (二重発注回避)。
            terminal = await self._await_order_terminal(
                ref.order_id, max_polls=_MODIFY_CANCEL_WAIT_POLLS
            )
            if terminal is None:
                return OrderResult(
                    status="REJECTED", filled_qty=0.0, avg_price=None,
                    client_order_id=order_id,
                    reject_reason="MODIFY_CANCEL_TIMEOUT:取消の確定を確認できませんでした",
                )

            # 3. 取消が成立するまでに約定した数量を差し引いた「残数量」だけ再発注する。
            #    取消は約定に勝てないことがあり (部分約定して残りが取消)、原数量をそのまま
            #    再発注すると約定済み分と二重建玉になる (over-fill = 実弾の発注事故、§2.2)。
            #    旧 leg の約定済み (filled_base 込み) は OrderEvent stream から消えないよう
            #    補償結果・remap 後の baseline に載せる (review HIGH)。
            already_filled = terminal.filled_qty
            # 論理注文の総目標数量。new_qty 指定時はそれ、無指定時は「これまでの累計約定
            # (filled_base) + 現 leg の発注数量 (ref.qty)」= 元の総目標を保つ。
            total_target = new_qty if new_qty is not None else ref.filled_base + ref.qty
            total_filled = ref.filled_base + already_filled
            merged_qty = total_target - total_filled
            merged_price = new_price if new_price is not None else ref.price
            # 旧 leg 込みの累計約定代金 → 加重平均価格 (約定ゼロなら None)。
            total_notional = ref.notional_base + already_filled * (terminal.avg_price or 0.0)
            total_avg = total_notional / total_filled if total_filled > 0 else None
            if merged_qty <= 0:
                # 取消確定までに目標数量を満たして約定済み → 再発注しない。同一
                # client_order_id を原注文の終端状態 (FILLED / CANCELED) で終端化する。
                self._unregister_order(order_id)
                # terminal.status は取消 leg 単体の判定。CumQty==0 かつ取消明細欠落だと
                # _terminal_zero_fill_status の既定 REJECTED が立つが、論理注文には
                # total_filled>0 の約定がある (filled_base 由来)。REJECTED は約定ゼロを
                # 含意するため自己矛盾 → CANCELED に丸める (約定済みは REJECTED にしない
                # 既存ルール order_status のミラー、fix #4)。
                final_status = terminal.status
                if total_filled > 0 and final_status != "FILLED":
                    final_status = "CANCELED"
                return OrderResult(
                    status=final_status,
                    filled_qty=total_filled,
                    avg_price=total_avg,
                    client_order_id=order_id,
                    reject_reason=(
                        None if final_status == "FILLED"
                        else "MODIFY_ALREADY_FILLED:原注文が目標数量まで約定済みのため再発注しません"
                    ),
                )
            new_payload = _orders.build_send_order_payload(
                symbol=ref.symbol,
                exchange=ref.exchange,
                side=ref.side,
                qty=merged_qty,
                price=merged_price,
                order_type=ref.order_type,
                time_in_force=ref.time_in_force,
                account_type=ref.account_type,
            )
            new_ack = await self._send_order(new_payload)
            if new_ack.rejected:
                if new_ack.reject_code == "-1":
                    # 再発注の異常終了 (システムエラー、§2.2): 新規注文の状態が不定なので
                    # KabuApiError を上層へ伝播しトーストで明示する。原注文は取消済みだが
                    # unregister せず、polling が CANCELED として後追い反映する。
                    raise KabuApiError(
                        -1, new_ack.reject_text or "kabu sendorder system error"
                    )
                # 取消成功 + 新規業務リジェクト: 元注文は取消済み。同一 client_order_id を
                # CANCELED 終端化する。約定済み (total_filled) があれば載せる — 旧 leg を
                # remap で捨てるため polling が後追いできず、ここで載せないと約定が消える。
                self._unregister_order(order_id)
                return OrderResult(
                    status="CANCELED",
                    filled_qty=total_filled,
                    avg_price=total_avg,
                    client_order_id=order_id,
                    reject_reason="MODIFY_NEW_FAILED:原注文は取消済みです。再発注してください",
                )

            # 4. 全成功: 同一 client_order_id に新 OrderId を再マップする。約定済み数量は
            #    filled_base に退避し、新しい原数量は再発注した残数量 (merged_qty)。次回
            #    modify と polling はこの baseline を基準に累計約定を算出する (約定が消えない)。
            self._order_id_to_cid.pop(ref.order_id, None)
            self._last_pushed.pop(order_id, None)
            ref.order_id = new_ack.order_id
            ref.qty = merged_qty
            ref.price = merged_price
            ref.filled_base = total_filled
            ref.notional_base = total_notional
            self._order_id_to_cid[new_ack.order_id] = order_id
            # 新 OrderId は非終端なので polling 継続が必要。poll task が (全注文終端で)
            # 自己終了済みでも再武装する。submit_order と対称 (fix #5)。
            self._ensure_orders_poll()
            return OrderResult(
                status="ACCEPTED",
                filled_qty=total_filled,
                avg_price=total_avg,
                client_order_id=order_id,
            )
        finally:
            self._modifying.discard(order_id)

    async def _fetch_wallet_cash(self) -> dict:
        await self._wallet_bucket.acquire()
        resp = await self._client.get(
            endpoint("wallet/cash", env=self._env),
            headers=auth_headers(self._token or ""),
            timeout=_ORDER_TIMEOUT,
        )
        data = resp.json()
        check_response(data, resp.status_code)
        return data

    async def _fetch_positions(self) -> list:
        await self._info_bucket.acquire()
        resp = await self._client.get(
            endpoint("positions", env=self._env),
            headers=auth_headers(self._token or ""),
            params={"product": 1, "addinfo": "true"},  # 現物のみ + 評価損益を含める
            timeout=_ORDER_TIMEOUT,
        )
        data = resp.json()
        check_response(data, resp.status_code)
        return data

    async def fetch_account(self) -> AccountSnapshot:
        """GET /wallet/cash (現物買付余力) + GET /positions (現物保有) で口座同期。

        2 本は独立で別 bucket (wallet 10/s・info 10/s) を引くため並行取得する
        (同期リフレッシュのレイテンシを半減。httpx.AsyncClient は並行安全)。
        """
        if self._token is None:
            raise RuntimeError("fetch_account requires login; call login() first")
        cash_data, pos_data = await asyncio.gather(
            self._fetch_wallet_cash(), self._fetch_positions()
        )
        # 現物口座は買付可能額 ≈ 利用可能現金。預り金専用 API は本 Step では使わない
        # (Tachibana fetch_account と同方針)。
        buying_power = _orders.parse_float(
            cash_data.get("StockAccountWallet") if isinstance(cash_data, dict) else 0
        )
        rows = pos_data if isinstance(pos_data, list) else []
        positions = tuple(
            AccountPositionData(
                symbol=str(p.get("Symbol", "")),
                qty=int(_orders.parse_float(p.get("LeavesQty"))),
                avg_price=_orders.parse_float(p.get("Price")),
                unrealized_pnl=_orders.parse_float(p.get("ProfitLoss")),
            )
            for p in rows
            if isinstance(p, dict)
            and _orders.parse_float(p.get("LeavesQty")) > 0  # 保有数量ゼロは除外
        )
        return AccountSnapshot(
            cash=buying_power, buying_power=buying_power, positions=positions
        )

    # ------------------------------------------------------------------
    # Venue Health Watchdog (Phase 9 §3.5 / Step 7)
    # ------------------------------------------------------------------

    async def check_health(self) -> bool:
        """GET /apisoftlimit を軽量 ping して本体ログイン状態を確認する (§3.5)。

        kabuステーション本体は早朝に強制ログアウトされる仕様 (kabusapi skill S1)。
        ログアウトすると REST は `4001007` / `4001017` (ログイン認証エラー) を返す。
        VenueHealthWatchdog が 30 秒間隔でこれを呼び、戻り値で再ログイン誘導を判断する。

        - **本体ログイン中** → ``True``。
        - **本体ログアウト / 未ログイン** (`4001007` / `4001017`) → ``False``
          (Watchdog が VenueLogoutDetected を push する)。
        - **transient 障害** (接続断・流量・その他 API エラー) → 例外を伝播する。
          Watchdog 側は best-effort で握り潰しバックオフするので、一過性の失敗で
          誤って再ログイン modal を出さない。

        ``GET /apisoftlimit`` を選ぶ理由 (§3.5): info 系 (10 req/sec) の最軽量エンドポイント
        で副作用が無い。``HEAD`` は `4001014 許可されていないHTTPメソッド` で失敗し、新規
        ``/token`` 発行は本体に負荷をかけるため使わない。
        """
        if self._token is None:
            # Watchdog は login 後にのみ起動・logout 前に停止されるため通常は到達しない。
            # teardown との race で token が消えた中間状態を「ログアウト検出」と誤認して
            # spurious な modal を出さないよう、transient 扱い (例外) にする。
            raise RuntimeError("check_health requires login; call login() first")
        await self._info_bucket.acquire()
        resp = await self._client.get(
            endpoint("apisoftlimit", env=self._env),
            headers=auth_headers(self._token),
            timeout=_ORDER_TIMEOUT,
        )
        data = resp.json()
        # ログアウト Code を check_response より先に判定する (check_response は logout も
        # 汎用 KabuApiError に丸めるため、ここで bool に変換しないと watchdog が transient と
        # 区別できない)。本体ログアウトは HTTP 200 + Code、または HTTP 401 + Code で来うる。
        code = data.get("Code") if isinstance(data, dict) else None
        if code in _VENUE_LOGGED_OUT_CODES:
            return False
        # ログアウト以外のエラー (流量 429・接続断・想定外 Code) は transient として伝播。
        check_response(data, resp.status_code)
        return True

    # ------------------------------------------------------------------
    # 約定確認 polling (GET /orders を 1 秒間隔、§3.3.2)
    # ------------------------------------------------------------------

    def _ensure_orders_poll(self) -> None:
        """OrderEvent push が設定済みなら polling task を 1 本起動する (idempotent)。"""
        if self._on_order_event is None:
            return
        if self._orders_poll_task is not None and not self._orders_poll_task.done():
            return
        self._orders_poll_task = asyncio.create_task(self._run_orders_poll())

    async def _stop_orders_poll(self) -> None:
        task = self._orders_poll_task
        if task is not None and not task.done():
            task.cancel()
            try:
                await task
            except asyncio.CancelledError:
                pass
            except Exception as exc:
                # 想定は CancelledError のみ。それ以外は polling task のシャットダウン時
                # バグなので握り潰さずログに残す (silent failure 回避)。
                logger.warning(
                    "kabu orders poll task errored during stop: %s", exc
                )
        self._orders_poll_task = None

    async def _run_orders_poll(self) -> None:
        """1 秒間隔で GET /orders を叩き、状態変化を OrderEvent に変換して push する。

        全注文が終端化して追跡対象がなくなったら自己終了する (idle な 1 秒ループを
        畳む)。次の ``submit_order`` が ``_ensure_orders_poll`` で再起動する。

        連続失敗時 (本体ログアウト / 401 / 接続断) は指数バックオフで間隔を延ばし、
        1Hz hot-loop と R5 流量浪費を避ける。成功で間隔は通常の 1 秒へ戻す。
        """
        backoff_s = 0.0
        while True:
            # 全注文終端で追跡対象が空 → idle ループを畳む。sleep の前に判定するので
            # backoff が伸びている最中に空になっても即終了する (task lingering 回避)。
            if not self._orders_ref:
                return
            try:
                await self._rate_limit_sleep(backoff_s or _ORDERS_POLL_INTERVAL_S)
            except asyncio.CancelledError:
                return
            try:
                await self._poll_orders_once()
                backoff_s = 0.0
            except asyncio.CancelledError:
                raise
            except BaseException as exc:  # noqa: BLE001 — best-effort: 1 回失敗で停止させない
                self._last_error = exc
                backoff_s = min(
                    (backoff_s or _ORDERS_POLL_INTERVAL_S) * 2, _POLL_MAX_BACKOFF_S
                )
                logger.warning(
                    "kabu orders poll failed, backing off %.0fs: %s", backoff_s, exc
                )

    async def _poll_orders_once(self) -> None:
        """GET /orders を 1 回叩き、追跡中注文の状態変化のみ push する。"""
        if self._token is None or self._on_order_event is None or not self._orders_ref:
            return
        await self._info_bucket.acquire()
        resp = await self._client.get(
            endpoint("orders", env=self._env),
            headers=auth_headers(self._token or ""),
            params={"product": 1},  # 現物のみ
            timeout=_ORDER_TIMEOUT,
        )
        data = resp.json()
        check_response(data, resp.status_code)
        for order in data if isinstance(data, list) else []:
            if not isinstance(order, dict):
                continue
            # 安価な ID 照合を先に行い、未追跡注文・訂正進行中の注文を parse 前に弾く。
            # GET /orders は口座の全注文を返すので、自分が出した注文だけ高コストな
            # parse_order_status (Details 走査・約定平均・時刻変換) を通す。
            cid = self._order_id_to_cid.get(str(order.get("ID", "")))
            if cid is None or cid in self._modifying:
                continue
            report = _orders.parse_order_status(order)
            if report is None:
                continue
            # 訂正で旧 leg を捨てた注文は filled_base に約定済みを退避している。論理注文の
            # 累計約定 = filled_base + 現 leg の CumQty。これを push しないと約定済みが消える。
            ref = self._orders_ref.get(cid)
            base_qty = ref.filled_base if ref is not None else 0.0
            base_notional = ref.notional_base if ref is not None else 0.0
            total_filled = base_qty + report.filled_qty
            status = report.status
            if base_qty > 0 and status == "ACCEPTED":
                # 旧 leg で約定済みがあるのに現 leg が未約定 = 論理的には部分約定状態。
                status = "PARTIALLY_FILLED"
            if total_filled > 0:
                avg_price = (base_notional + report.filled_qty * report.avg_price) / total_filled
            else:
                avg_price = report.avg_price
            key = (status, total_filled)
            if self._last_pushed.get(cid) == key:
                continue  # 状態・約定量に変化なし
            self._last_pushed[cid] = key
            ts_ms = report.ts_ms or int(_time_module.time() * 1000)
            self._on_order_event(
                OrderEventData(
                    order_id=cid,
                    venue_order_id=report.order_id,
                    client_order_id=cid,
                    status=status,
                    filled_qty=total_filled,
                    avg_price=avg_price,
                    ts_ms=ts_ms,
                )
            )
            # 終端注文は以降ポーリング不要 — レジストリから外して poll を軽量に保つ
            # (全注文が終端化すれば _poll_orders_once は HTTP を叩かず即 return)。
            if report.terminal:
                self._unregister_order(cid)

    async def _await_order_terminal(
        self, order_id: str, *, max_polls: int
    ) -> "_orders.OrderStatusReport | None":
        """GET /orders?id=... を polling し、対象注文が終端 (State==5) になったら、その
        確定時点の ``OrderStatusReport`` を返す。確認できなければ ``None``。

        返り値の ``filled_qty`` を訂正 (取消→新規) の残数量算出に使うため bool ではなく
        report を返す (full-qty 再発注による over-fill 回避、§2.2)。
        """
        for i in range(max_polls):
            await self._info_bucket.acquire()
            resp = await self._client.get(
                endpoint("orders", env=self._env),
                headers=auth_headers(self._token or ""),
                params={"product": 1, "id": order_id},
                timeout=_ORDER_TIMEOUT,
            )
            data = resp.json()
            check_response(data, resp.status_code)
            # /orders は配列が正だが、単一 dict 応答も防御的に受ける (空応答は [] 扱い)。
            rows = data if isinstance(data, list) else [data] if isinstance(data, dict) else []
            for order in rows:
                if not isinstance(order, dict):
                    continue
                report = _orders.parse_order_status(order)
                if report is not None and report.order_id == order_id and report.terminal:
                    return report
            if i < max_polls - 1:
                await self._rate_limit_sleep(_ORDERS_POLL_INTERVAL_S)
        return None
