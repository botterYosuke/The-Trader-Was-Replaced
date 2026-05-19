"""Phase 8 §1.3 LiveVenueAdapter の Tachibana 実装骨格。HTTP/WS は後続 step。"""

from __future__ import annotations

import asyncio
import os
from typing import AsyncIterator, Literal

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
from engine.exchanges._env_guard import require_prod_env
from engine.exchanges.tachibana_auth import (
    ApiError,
    PNoCounter,
    TachibanaSession,
    check_response as _auth_check_response,
    login as _auth_login,
)
from engine.exchanges.tachibana_master import (
    MasterStreamParser,
    build_instruments_from_master_records,
)
from engine.exchanges.tachibana_url import EventUrl, build_event_url
from engine.exchanges.tachibana_ws import FdFrameProcessor, TickerEventWsHub

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
        source = creds.credentials_source
        if source == "session_cache":
            from engine.exchanges.tachibana_file_store import load_session, is_session_valid_for_today
            data = load_session()
            if data is None:
                raise ValueError("SESSION_CACHE_MISSING")
            if not is_session_valid_for_today(data):
                raise ValueError("SESSION_CACHE_EXPIRED")
            self._apply_session_from_data(data)
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

    async def logout(self) -> None:
        for hub in list(self._hubs.values()):
            await hub.aclose()
        self._hubs.clear()
        self._processors.clear()
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
