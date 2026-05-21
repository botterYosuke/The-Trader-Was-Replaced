"""Phase 8 §1.3 LiveVenueAdapter の Tachibana 実装骨格。HTTP/WS は後続 step。"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import uuid
from dataclasses import dataclass
from typing import AsyncIterator, Awaitable, Callable, Literal, Protocol

import httpx

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
from engine.exchanges import tachibana_orders as _orders
from engine.exchanges._env_guard import require_prod_env
from engine.exchanges.tachibana_auth import (
    ApiError,
    PNoCounter,
    TachibanaSession,
    check_response as _auth_check_response,
    current_p_sd_date,
    login as _auth_login,
)
from engine.exchanges.tachibana_codec import (
    decode_response_body,
    deserialize_tachibana_list,
)
from engine.exchanges.tachibana_master import (
    MasterStreamParser,
    build_instruments_from_master_records,
)
from engine.exchanges.tachibana_url import (
    EventUrl,
    RequestUrl,
    build_event_url,
    build_request_url,
)
from engine.exchanges.tachibana_ws import (
    FdFrameProcessor,
    TachibanaEventWs,
    TickerEventWsHub,
)

log = logging.getLogger(__name__)

# REQUEST I/F (発注・余力・保有) は master DL より軽量。read 30s で十分。
_REQUEST_TIMEOUT = httpx.Timeout(connect=10.0, read=30.0, write=10.0, pool=5.0)


class _SecretResolver(Protocol):
    async def resolve(self, venue: str, purpose: str) -> str: ...


# on_order_event(OrderEventData) -> None : EC 約定通知を proto 化して push する
# transport コールバック (server_grpc が publish_backend_event に束ねて注入)。
OnOrderEvent = Callable[[OrderEventData], None]

# on_venue_logout(venue: str) -> None : 本体ログアウト (SS=閉局) を UI に push する
# transport コールバック (Phase 9 §3.5 / Step 7)。server_grpc が VenueLogoutDetected に
# 束ねて注入する。kabu は poll 型 watchdog で検知するが Tachibana は SS フレームで push 検知。
OnVenueLogout = Callable[[str], None]


@dataclass(frozen=True)
class _TachibanaOrderRef:
    """発注済み注文の venue 識別子 (取消/訂正で再供給する)。

    Tachibana の取消/訂正は ``sOrderNumber`` + ``sEigyouDay`` の 2 識別子が必須。
    facade は client_order_id しか持たないため、adapter が内部で対応付けを保持する
    (proto OrderEvent に order_date を足さずに済ませる Step 5 の設計判断)。
    """

    client_order_id: str
    order_number: str
    eigyou_day: str
    issue_code: str
    qty: float  # 発注数量。EC の残数量から累計約定数量を導出するのに使う。

# Phase 8 §3.2: env-based credential keys (tachibana skill §S2).
# 第二暗証番号 (s_second_password) は env に置かない (handoff 制約)。
_ENV_USER_ID = "DEV_TACHIBANA_USER_ID"
_ENV_PASSWORD = "DEV_TACHIBANA_PASSWORD"

# Master DL (CLMEventDownload) returns the entire instrument universe — for
# kabu master this is multi-MB and can stream for several minutes on a slow
# connection. The original 60s read timeout consistently aborted mid-stream on
# residential links. We raise the read timeout to a value comfortably above
# the observed worst case (≈4 min on a
# loaded mobile tether) while still bounding indefinite hangs.
_MASTER_READ_TIMEOUT = 600.0


class TachibanaAdapter:
    venue_id: str = "TACHIBANA"

    def __init__(self, environment: Literal["demo", "prod"] = "demo"):
        if environment not in ("demo", "prod"):
            raise ValueError("environment must be 'demo' or 'prod'")
        self._env = environment
        # R4: PNoCounter は adapter で 1 個保持し、retry / re-login で共有する。
        self._p_no_counter = PNoCounter()
        self._session: TachibanaSession | None = None
        # Phase 8 §3.2 A3.3: per-ticker WS hub registry.
        self._hubs: dict[str, TickerEventWsHub] = {}
        self._processors: dict[str, FdFrameProcessor] = {}
        self._queue: asyncio.Queue[LiveEvent] = asyncio.Queue()
        # Phase 9 Step 5: 発注経路 (set_execution_hooks で注入)。
        self._secret_resolver: _SecretResolver | None = None
        self._on_order_event: OnOrderEvent | None = None
        # Phase 9 Step 7: 本体ログアウト (SS=閉局) 検知 callback (set_execution_hooks で注入)。
        self._on_venue_logout: OnVenueLogout | None = None
        # SS フレームの直近システム状態。閉局 (sSystemStatus="0") への遷移を一度だけ通知
        # するための debounce 用 (SS は接続毎に初回再送されるため毎フレーム通知すると連打)。
        self._last_system_open: bool | None = None
        # client_order_id -> venue 識別子。取消/訂正・EC 解決に使う。
        self._orders_ref: dict[str, _TachibanaOrderRef] = {}
        self._order_number_to_cid: dict[str, str] = {}
        # EC は接続毎に当日分を全件再送するため、(venue_order_id, trade_id,
        # notification_type) の seen-set で重複 push を抑止する (e-station C-H3 流儀)。
        self._seen_ec: set[tuple[str, str, str]] = set()
        # 口座レベル EC (約定通知) WS。login で起動・logout で停止する。
        self._ec_ws: TachibanaEventWs | None = None
        self._ec_task: asyncio.Task | None = None
        self._ec_stop: asyncio.Event | None = None

    @property
    def is_logged_in(self) -> bool:
        return self._session is not None

    def _apply_session_from_data(self, data: dict) -> None:
        """Populate self._session and advance p_no from a loaded session dict."""
        from engine.exchanges.tachibana_url import RequestUrl, MasterUrl, PriceUrl, EventUrl
        self._session = TachibanaSession(
            url_request=RequestUrl(data["url_request"]),
            url_master=MasterUrl(data["url_master"]),
            url_price=PriceUrl(data["url_price"]),
            url_event=EventUrl(data["url_event"]),
            url_event_ws=data["url_event_ws"],
            zyoutoeki_kazei_c=data.get("zyoutoeki_kazei_c", ""),
        )
        last_p_no = data.get("last_p_no")
        if isinstance(last_p_no, int):
            self._p_no_counter.fast_forward(last_p_no)

    async def login(self, creds: VenueCredentials) -> None:
        """Resolve credentials per `creds.credentials_source` and call auth.login()."""
        # Recreate the queue rather than draining: a prior logout() enqueued a
        # None sentinel that would terminate the next session's events() consumer.
        # Any pending producer task holding the old queue is also severed.
        self._queue = asyncio.Queue()
        # Re-login without an intervening logout(): tear down the prior EC stream
        # and order registry so stale notifications / id mappings don't bleed
        # across sessions.
        await self._stop_ec_stream()
        self._orders_ref.clear()
        self._order_number_to_cid.clear()
        self._seen_ec.clear()
        self._last_system_open = None  # SS 閉局 debounce を新セッションでリセット
        source = creds.credentials_source
        if source == "session_cache":
            from engine.exchanges.tachibana_file_store import load_session, is_session_valid_for_today
            data = load_session()
            if data is None:
                raise ValueError("SESSION_CACHE_MISSING")
            if not is_session_valid_for_today(data):
                raise ValueError("SESSION_CACHE_EXPIRED")
            self._apply_session_from_data(data)
            self._ensure_ec_stream()
            return
        if source == "prompt":
            # run_dialog() persists the session to disk on success so we reload
            # from the file (session_cache path). Offloaded to a thread because
            # tkinter mainloop blocks — keeping the asyncio loop responsive.
            from engine.exchanges import tachibana_login_flow
            from engine.exchanges.tachibana_file_store import is_session_valid_for_today, load_session
            result = await asyncio.to_thread(
                tachibana_login_flow.run_dialog, env_hint=self._env
            )
            if not result.get("success"):
                error_code = str(result.get("error_code") or "USER_CANCELLED")
                raise ValueError(error_code)
            data = load_session()
            if data is None or not is_session_valid_for_today(data):
                # run_dialog reported success but save_session did not land —
                # defensive guard against an unexpected race / file-system failure.
                raise ValueError("PROMPT_SESSION_MISSING")
            self._apply_session_from_data(data)
            self._ensure_ec_stream()
            return
        if source != "env":
            raise ValueError(f"unknown credentials_source: {source!r}")

        user_id = os.environ.get(_ENV_USER_ID)
        password = os.environ.get(_ENV_PASSWORD)
        if not user_id or not password:
            # R10: do NOT include the values themselves (only the key names).
            missing = [
                k for k, v in ((_ENV_USER_ID, user_id), (_ENV_PASSWORD, password))
                if not v
            ]
            raise ValueError(
                f"missing env credentials: {', '.join(missing)} "
                f"(credentials_source='env')"
            )

        is_demo = self._env == "demo"
        if not is_demo:
            # Production double-guard (R1 / spec). require_prod_env raises
            # RuntimeError if TACHIBANA_ALLOW_PROD != '1'.
            require_prod_env("TACHIBANA_ALLOW_PROD")

        self._session = await _auth_login(
            user_id,
            password,
            is_demo=is_demo,
            p_no_counter=self._p_no_counter,
        )
        self._ensure_ec_stream()

    async def logout(self) -> None:
        await self._stop_ec_stream()
        for hub in list(self._hubs.values()):
            await hub.aclose()
        self._hubs.clear()
        self._processors.clear()
        self._orders_ref.clear()
        self._order_number_to_cid.clear()
        self._seen_ec.clear()
        self._last_system_open = None  # SS 閉局 debounce を新セッションでリセット
        self._session = None
        # Wake any active events() consumer so it sees StopAsyncIteration
        # instead of hanging on queue.get() forever.
        self._queue.put_nowait(None)  # type: ignore[arg-type]

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        """CLMEventDownload で master record を一括取得し InstrumentRaw に集約する。

        Phase 8 §3.2 A2.3b: MVP 実装。
        - sUrlMaster + CLMEventDownload (sJsonOfmt='4')
        - record stream は SJIS decode 後 JSONDecoder.raw_decode で 1 件ずつ取り出す
        - sCLMID で 3 種に振り分け: CLMIssueMstKabu / CLMIssueSizyouMstKabu / CLMYobine
        - 終端 CLMEventDownloadComplete までを 1 バッチとして処理
        """
        if self._session is None:
            raise RuntimeError(
                "fetch_instruments requires an active session; call login() first"
            )

        from engine.exchanges.tachibana_auth import current_p_sd_date
        from engine.exchanges.tachibana_url import build_request_url

        payload = {
            "p_no": str(self._p_no_counter.next()),
            "p_sd_date": current_p_sd_date(),
            "sCLMID": "CLMEventDownload",
            "sTargetCLMID": "CLMIssueMstKabu,CLMIssueSizyouMstKabu,CLMYobine",
        }
        url = build_request_url(self._session.url_master, payload, sJsonOfmt="4")

        _TIMEOUT = httpx.Timeout(
            connect=10.0, read=_MASTER_READ_TIMEOUT, write=10.0, pool=5.0
        )
        parser = MasterStreamParser()
        async with httpx.AsyncClient(timeout=_TIMEOUT) as client:
            async with client.stream("GET", url) as resp:
                resp.raise_for_status()
                async for chunk in resp.aiter_bytes():
                    # SJIS decoder is errors="strict" (R7) — wrap UnicodeDecodeError
                    # so callers see a typed ApiError rather than a raw exception.
                    try:
                        parser.feed(chunk)
                    except UnicodeDecodeError as exc:
                        raise ApiError(
                            "MASTER_DECODE_FAILED", str(exc)
                        ) from exc
                    if parser.is_complete:
                        break

        records = parser.records()
        # An error envelope (p_errno / sResultCode) arrives without the
        # CLMEventDownloadComplete terminator. It may not be the first record —
        # scan the full list and run the R6 two-stage check on the first match.
        if not parser.is_complete:
            for rec in records:
                if isinstance(rec, dict) and (
                    "p_errno" in rec or "sResultCode" in rec
                ):
                    _auth_check_response(rec)
                    break
        return build_instruments_from_master_records(records)

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        # §9.5 ADR: channels は accept-and-ignore（trades + depth 固定）
        if self._session is None:
            raise RuntimeError(
                "subscribe requires an active session; call login() first"
            )
        ticker = instrument_id.split(".")[0]
        processor = self._processors.get(ticker)
        if processor is None:
            processor = FdFrameProcessor(row="1")
            self._processors[ticker] = processor
        hub = self._hubs.get(ticker)
        if hub is None:
            # Phase 8 §3.2 A3.3 review fix (High): EVENT WS は必須クエリを
            # build_event_url で組み立てる。市場コードは MVP "00" 固定
            # (TSE 想定)。名証/福証/札証対応時は master lookup へ。
            ws_url = build_event_url(
                EventUrl(self._session.url_event_ws),
                {
                    "p_rid": "22",
                    "p_board_no": "1000",
                    "p_gyou_no": "1",
                    "p_issue_code": ticker,
                    "p_mkt_code": "00",
                    "p_eno": "0",
                    "p_evt_cmd": "ST,KP,FD",
                },
            )
            hub = TickerEventWsHub(
                ws_url,
                ticker=ticker,
            )
            self._hubs[ticker] = hub
        await hub.subscribe(
            instrument_id,
            self._make_callback(instrument_id, processor),
            on_connect=processor.reset,
        )

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        ticker = instrument_id.split(".")[0]
        hub = self._hubs.get(ticker)
        if hub is None:
            return
        await hub.unsubscribe(instrument_id)
        if hub.subscriber_count == 0:
            await hub.aclose()
            self._hubs.pop(ticker, None)
            self._processors.pop(ticker, None)

    async def events(self) -> AsyncIterator[LiveEvent]:
        while True:
            item = await self._queue.get()
            if item is None:  # None sentinel from logout() signals normal termination
                return
            yield item

    def _make_callback(
        self, instrument_id: InstrumentId, processor: FdFrameProcessor
    ):
        async def _cb(frame_type: str, fields: dict, recv_ts_ms: int) -> None:
            if frame_type != "FD":
                return
            trade, depth = processor.process(fields, recv_ts_ms)
            if depth is not None:
                ts_ns = int(depth["recv_ts_ms"]) * 1_000_000
                bids = tuple(
                    DepthLevel(price=float(lv["price"]), size=float(lv["qty"]))
                    for lv in depth["bids"]
                )
                asks = tuple(
                    DepthLevel(price=float(lv["price"]), size=float(lv["qty"]))
                    for lv in depth["asks"]
                )
                self._queue.put_nowait(
                    DepthUpdate(
                        kind="depth",
                        instrument_id=instrument_id,
                        ts_ns=ts_ns,
                        bids=bids,
                        asks=asks,
                    )
                )
            if trade is not None and trade["side"] != "unknown":
                self._queue.put_nowait(
                    TradesUpdate(
                        kind="trades",
                        instrument_id=instrument_id,
                        ts_ns=int(trade["ts_ms"]) * 1_000_000,
                        price=float(trade["price"]),
                        size=float(trade["qty"]),
                        aggressor_side=trade["side"],
                    )
                )
        return _cb

    # ------------------------------------------------------------------
    # Phase 9 Step 5: OrderingVenueAdapter — 発注 / 取消 / 訂正 / 口座
    # ------------------------------------------------------------------

    def set_execution_hooks(
        self,
        *,
        secret_resolver: _SecretResolver,
        on_order_event: OnOrderEvent,
        on_venue_logout: OnVenueLogout | None = None,
    ) -> None:
        """server_grpc が secret 解決・OrderEvent push・ログアウト検知を注入する。

        ``login()`` より前に呼ぶこと (EC ストリームは on_order_event 設定済みの
        ときだけ起動するため)。既にログイン済みなら EC ストリームをここで起動する。
        ``on_venue_logout`` は EVENT WS の SS=閉局フレームから呼ばれる (§3.5 / Step 7)。
        """
        self._secret_resolver = secret_resolver
        self._on_order_event = on_order_event
        self._on_venue_logout = on_venue_logout
        if self._session is not None:
            try:
                self._ensure_ec_stream()
            except RuntimeError:
                # 実行中ループが無い (同期コンテキスト) ときは login が後で起動する。
                pass

    async def _resolve_secret(self, purpose: str) -> str:
        if self._secret_resolver is None:
            raise RuntimeError(
                "second-password resolver not configured; call set_execution_hooks()"
            )
        return await self._secret_resolver.resolve(self.venue_id, purpose)

    async def _request(self, payload: dict[str, object]) -> dict:
        """REQUEST I/F (sUrlRequest) に GET し SJIS→JSON で応答 dict を返す。

        p_no / p_sd_date を R4 に従って付与する (build_request_url が sJsonOfmt='5'
        と立花独自 percent-encode を担う)。
        """
        if self._session is None:
            raise RuntimeError("requires an active session; call login() first")
        body: dict[str, object] = {
            "p_no": str(self._p_no_counter.next()),
            "p_sd_date": current_p_sd_date(),
            **payload,
        }
        url = build_request_url(
            RequestUrl(str(self._session.url_request)), body, sJsonOfmt="5"
        )
        async with httpx.AsyncClient(timeout=_REQUEST_TIMEOUT) as client:
            resp = await client.get(url)
            resp.raise_for_status()
            return json.loads(decode_response_body(resp.content))

    @staticmethod
    def _rejected_result(client_order_id: str, ack: "_orders.OrderAck") -> OrderResult:
        """業務リジェクト (sResultCode != 0) を REJECTED な OrderResult に正規化する。"""
        return OrderResult(
            status="REJECTED", filled_qty=0.0, avg_price=None,
            client_order_id=client_order_id,
            reject_reason=f"{ack.reject_code}:{ack.reject_text}",
        )

    def _register_order(
        self, client_order_id: str, order_number: str, eigyou_day: str,
        issue_code: str, qty: float,
    ) -> None:
        self._orders_ref[client_order_id] = _TachibanaOrderRef(
            client_order_id=client_order_id,
            order_number=order_number,
            eigyou_day=eigyou_day,
            issue_code=issue_code,
            qty=qty,
        )
        if order_number:
            self._order_number_to_cid[order_number] = client_order_id

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
        """CLMKabuNewOrder で新規発注する (sSecondPassword を都度収集)。"""
        if self._session is None:
            raise RuntimeError("submit_order requires an active session; call login() first")
        issue_code = instrument_id.split(".")[0]
        second_password = await self._resolve_secret("new_order")
        payload = _orders.build_new_order_payload(
            issue_code=issue_code,
            side=side,
            qty=qty,
            price=price,
            order_type=order_type,
            time_in_force=time_in_force,
            second_password=second_password,
            zyoutoeki_kazei_c=self._session.zyoutoeki_kazei_c,
        )
        ack = _orders.parse_order_response(await self._request(payload))
        client_order_id = uuid.uuid4().hex
        if ack.rejected:
            return self._rejected_result(client_order_id, ack)
        self._register_order(
            client_order_id, ack.order_number, ack.eigyou_day, issue_code, qty
        )
        # 新規受付。約定 (FILLED/PARTIALLY_FILLED) は EC 通知で後追いする。
        return OrderResult(
            status="ACCEPTED", filled_qty=0.0, avg_price=None,
            client_order_id=client_order_id,
        )

    async def cancel_order(
        self, *, venue: str, order_id: str, **extra: object
    ) -> OrderResult:
        """CLMKabuCancelOrder で取消する (sSecondPassword 必須・2 識別子を再供給)。"""
        ref = self._orders_ref.get(order_id)
        if ref is None:
            return OrderResult(
                status="REJECTED", filled_qty=0.0, avg_price=None,
                client_order_id=order_id, reject_reason="UNKNOWN_VENUE_ORDER",
            )
        second_password = await self._resolve_secret("cancel_order")
        payload = _orders.build_cancel_order_payload(
            order_number=ref.order_number,
            eigyou_day=ref.eigyou_day,
            second_password=second_password,
        )
        ack = _orders.parse_order_response(await self._request(payload))
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
        """CLMKabuCorrectOrder で訂正する (atomic・sSecondPassword 必須)。"""
        ref = self._orders_ref.get(order_id)
        if ref is None:
            return OrderResult(
                status="REJECTED", filled_qty=0.0, avg_price=None,
                client_order_id=order_id, reject_reason="UNKNOWN_VENUE_ORDER",
            )
        second_password = await self._resolve_secret("correct_order")
        payload = _orders.build_correct_order_payload(
            order_number=ref.order_number,
            eigyou_day=ref.eigyou_day,
            second_password=second_password,
            new_price=new_price,
            new_qty=new_qty,
        )
        ack = _orders.parse_order_response(await self._request(payload))
        if ack.rejected:
            return self._rejected_result(order_id, ack)
        return OrderResult(
            status="ACCEPTED", filled_qty=0.0, avg_price=None, client_order_id=order_id,
        )

    async def fetch_account(self) -> AccountSnapshot:
        """CLMZanKaiKanougaku (買余力) + CLMGenbutuKabuList (現物保有) で口座同期。"""
        if self._session is None:
            raise RuntimeError("fetch_account requires an active session; call login() first")
        bp_resp = await self._request(
            {"sCLMID": "CLMZanKaiKanougaku", "sIssueCode": "", "sSizyouC": ""}
        )
        _auth_check_response(bp_resp)
        buying_power = _orders.parse_float(bp_resp.get("sSummaryGenkabuKaituke"))

        pos_resp = await self._request({"sCLMID": "CLMGenbutuKabuList", "sIssueCode": ""})
        _auth_check_response(pos_resp)
        raw = deserialize_tachibana_list(pos_resp.get("aGenbutuKabuList", ""))
        positions = tuple(
            AccountPositionData(
                symbol=str(p.get("sUriOrderIssueCode", "")),
                qty=int(_orders.parse_float(p.get("sUriOrderZanKabuSuryou"))),
                avg_price=_orders.parse_float(p.get("sUriOrderGaisanBokaTanka")),
                unrealized_pnl=_orders.parse_float(p.get("sUriOrderGaisanHyoukaSoneki")),
            )
            for p in raw
            if isinstance(p, dict)
        )
        # 現物口座は買付可能額 ≈ 利用可能現金。専用の預り金 API は本 Step では使わない
        # (計画 §3.4 は CLMZanKaiKanougaku + CLMGenbutuKabuList の 2 本のみ規定)。
        return AccountSnapshot(
            cash=buying_power, buying_power=buying_power, positions=positions
        )

    # ------------------------------------------------------------------
    # 口座レベル EC (注文約定通知) ストリーム
    # ------------------------------------------------------------------

    def _ensure_ec_stream(self) -> None:
        """口座レベルの EC WS を 1 本だけ起動する (hooks 設定済み & session 有時)。

        FD (時価) の per-ticker hub とは別。EC は口座単位で接続毎に全件再送される
        ため、ticker 購読とは独立に 1 本維持する。on_order_event 未設定 (mock/kabu)
        では起動しない。
        """
        if self._on_order_event is None or self._session is None:
            return
        if self._ec_task is not None and not self._ec_task.done():
            return
        # ⚠️ TENTATIVE: 口座レベル EC URL のクエリ構成 (issue 非依存) は実 Demo で
        # 要検証 (api_event_if.xlsx / 計画 §5.1 layer-3)。FD と同じ build_event_url
        # を使い、p_evt_cmd に EC/SS/US を含める。
        ws_url = build_event_url(
            EventUrl(str(self._session.url_event_ws)),
            {
                "p_rid": "22",
                "p_board_no": "1000",
                "p_eno": "0",
                "p_evt_cmd": "ST,KP,EC,SS,US",
            },
        )
        self._ec_stop = asyncio.Event()
        self._ec_ws = TachibanaEventWs(ws_url, self._ec_stop, ticker="EVENT")
        self._ec_task = asyncio.create_task(self._ec_ws.run(self._dispatch_event_frame))

    async def _stop_ec_stream(self) -> None:
        if self._ec_stop is not None:
            self._ec_stop.set()
        task = self._ec_task
        if task is not None and not task.done():
            try:
                await asyncio.wait_for(task, timeout=2.0)
            except asyncio.TimeoutError:
                task.cancel()
                try:
                    await task
                except (asyncio.CancelledError, Exception):
                    pass
            except (asyncio.CancelledError, Exception):
                pass
        self._ec_task = None
        self._ec_ws = None
        self._ec_stop = None

    def _handle_system_status(self, fields: dict[str, str]) -> None:
        """SS=システムステータス (CLMSystemStatus) を読み本体ログアウト/閉局を検知する (§3.5)。

        ⚠️ **TENTATIVE (要 Demo 検証 = 計画 §5.1 layer-3)**: SS は EVENT WS で配信される
        CLMSystemStatus マスタレコードだが、EVENT フレームでのフィールド名 prefix
        (``sSystemStatus`` か ``p_*`` 変種か) は実 Demo で未確認。EC 購読 URL / comma
        エンコードと同じ Demo-pending 事項。判別フィールド欠落時は安全側 (= 通知しない)。

        CLMSystemStatus (mfds_json_api_ref):
          ``sSystemStatus``    システム状態     ``0``:閉局 / ``1``:開局 / ``2``:一時停止
          ``sLoginKyokaKubun`` ログイン許可区分  ``0``:不許可 / ``1``:許可 / ``2``:不許可(時間外) / ``9``:管理者のみ

        閉局 (``sSystemStatus != "1"``) か ログイン不許可 (``sLoginKyokaKubun`` not in
        ``{"1","9"}``) を「本体ログアウト → 要再ログイン」とみなす。SS は接続毎に初回再送
        されるため、open→closed の遷移時 (または初回観測が closed) のみ 1 回通知する。
        """
        system_status = fields.get("sSystemStatus")
        login_kubun = fields.get("sLoginKyokaKubun")
        if system_status is None and login_kubun is None:
            return  # SS と判別できるフィールドが無い → prefix 不一致等。安全側で無視。
        is_open = system_status == "1" and (
            login_kubun is None or login_kubun in ("1", "9")
        )
        prev_open = self._last_system_open
        self._last_system_open = is_open
        if is_open:
            return  # 開局 → debounce 解除 (次の閉局でまた通知できる)。
        if prev_open is False:
            return  # 既に閉局通知済み (SS 再送) → 連打しない。
        if self._on_venue_logout is not None:
            self._on_venue_logout(self.venue_id)

    async def _dispatch_event_frame(
        self, frame_type: str, fields: dict[str, str], recv_ts_ms: int
    ) -> None:
        """EC を OrderEvent に、SS=システムステータスを閉局検知に回す (KP/ST/US は無視)。"""
        if frame_type == "SS":
            self._handle_system_status(fields)
            return
        if frame_type != "EC" or self._on_order_event is None:
            return
        report = _orders.parse_ec_frame(fields)
        if report is None:
            return
        # EC は再接続毎に当日分を全件再送する。(venue_order_id, trade_id,
        # notification_type) の seen-set で再送をスキップする (新規イベントのみ push)。
        seen_key = (report.venue_order_id, report.trade_id, report.notification_type)
        if seen_key in self._seen_ec:
            return
        self._seen_ec.add(seen_key)

        status = _orders.ec_status(report.notification_type, report.leaves_qty)
        client_order_id = self._order_number_to_cid.get(report.venue_order_id, "")
        ref = self._orders_ref.get(client_order_id)
        # 累計約定数量: 発注数量 - 残数量 (両方既知時)。未知なら今回約定分で代替。
        if ref is not None and report.leaves_qty is not None:
            filled_qty = max(0.0, ref.qty - report.leaves_qty)
        elif report.last_qty is not None:
            filled_qty = report.last_qty
        else:
            filled_qty = 0.0
        event = OrderEventData(
            order_id=client_order_id or report.venue_order_id,
            venue_order_id=report.venue_order_id,
            client_order_id=client_order_id,
            status=status,
            filled_qty=filled_qty,
            avg_price=report.last_price if report.last_price is not None else 0.0,
            ts_ms=report.ts_event_ms if report.ts_event_ms else recv_ts_ms,
        )
        self._on_order_event(event)
