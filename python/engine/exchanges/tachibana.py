"""Phase 8 §1.3 LiveVenueAdapter の Tachibana 実装骨格。HTTP/WS は後続 step。"""

from __future__ import annotations

import json
import os
from typing import AsyncIterator, Literal

import httpx

from engine.live.adapter import (
    Channel,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    VenueCredentials,
)
from engine.exchanges._env_guard import require_prod_env
from engine.exchanges.tachibana_auth import (
    PNoCounter,
    TachibanaSession,
    login as _auth_login,
)

# Phase 8 §3.2: env-based credential keys (tachibana skill §S2).
# 第二暗証番号 (s_second_password) は env に置かない (handoff 制約)。
_ENV_USER_ID = "DEV_TACHIBANA_USER_ID"
_ENV_PASSWORD = "DEV_TACHIBANA_PASSWORD"


class TachibanaAdapter:
    venue_id: str = "TACHIBANA"

    def __init__(self, environment: Literal["demo", "prod"] = "demo"):
        if environment not in ("demo", "prod"):
            raise ValueError("environment must be 'demo' or 'prod'")
        self._env = environment
        # R4: PNoCounter は adapter で 1 個保持し、retry / re-login で共有する。
        self._p_no_counter = PNoCounter()
        self._session: TachibanaSession | None = None

    async def login(self, creds: VenueCredentials) -> None:
        """Resolve credentials per `creds.credentials_source` and call auth.login().

        MVP (Phase 8 §3.2 A1.5): `env` のみ実装。
        `session_cache` / `prompt` は後続 step。
        """
        source = creds.credentials_source
        if source == "session_cache":
            raise NotImplementedError(
                "credentials_source='session_cache' は後続 step で実装 (Phase 8 §3.2)"
            )
        if source == "prompt":
            raise NotImplementedError(
                "credentials_source='prompt' は後続 step で実装 (Phase 8 §3.2)"
            )
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
        self._session = None

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
        from engine.exchanges.tachibana_codec import decode_response_body
        from engine.exchanges.tachibana_url import build_request_url

        payload = {
            "p_no": str(self._p_no_counter.next()),
            "p_sd_date": current_p_sd_date(),
            "sCLMID": "CLMEventDownload",
            "sTargetCLMID": "CLMIssueMstKabu,CLMIssueSizyouMstKabu,CLMYobine",
        }
        url = build_request_url(self._session.url_master, payload, sJsonOfmt="4")

        _TIMEOUT = httpx.Timeout(connect=10.0, read=60.0, write=10.0, pool=5.0)
        async with httpx.AsyncClient(timeout=_TIMEOUT) as client:
            resp = await client.get(url)
            resp.raise_for_status()
            body = resp.content

        text = decode_response_body(body)

        # JSON object stream を 1 件ずつ取り出す
        decoder = json.JSONDecoder()
        issues: dict[str, dict] = {}   # code -> {name}
        sizyou: dict[str, dict] = {}   # code -> {market, yobine_tani, lot_size}
        yobine_tables: dict[str, float] = {}  # yobine_tani -> tick_size (1段目)

        idx = 0
        n = len(text)
        while idx < n:
            # whitespace skip
            while idx < n and text[idx] in " \t\r\n":
                idx += 1
            if idx >= n:
                break
            rec, end = decoder.raw_decode(text, idx)
            idx = end
            if not isinstance(rec, dict):
                continue
            clmid = rec.get("sCLMID")
            if clmid == "CLMEventDownloadComplete":
                break
            if clmid == "CLMIssueMstKabu":
                issues[rec["sIssueCode"]] = {"name": rec.get("sIssueName", "")}
            elif clmid == "CLMIssueSizyouMstKabu":
                sizyou[rec["sIssueCode"]] = {
                    "market": rec.get("sSizyouC", ""),
                    "yobine_tani": rec.get("sYobineTaniNumber", ""),
                    "lot_size": int(rec.get("sBaibaiTaniNumber") or 0),
                }
            elif clmid == "CLMYobine":
                tani = rec.get("sYobineTaniNumber", "")
                tanka = float(rec.get("sYobineTanka_1") or 0)
                decimal = int(rec.get("sDecimal_1") or 0)
                yobine_tables[tani] = tanka / (10 ** decimal) if decimal else tanka

        out: list[InstrumentRaw] = []
        for code, sz in sizyou.items():
            iss = issues.get(code)
            if iss is None:
                continue
            tick = yobine_tables.get(sz["yobine_tani"], 0.0)
            out.append(InstrumentRaw(
                code=code,
                name=iss["name"],
                market=sz["market"],
                tick_size=tick,
                lot_size=sz["lot_size"],
            ))
        return out

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        raise NotImplementedError("Phase 8 後半 WS step で実装")

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("Phase 8 後半 WS step で実装")

    def events(self) -> AsyncIterator[LiveEvent]:
        raise NotImplementedError("Phase 8 後半 WS step で実装")
