"""Phase 8 §1.3 LiveVenueAdapter の Tachibana 実装骨格。HTTP/WS は後続 step。"""

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
        raise NotImplementedError("Phase 8 後半 HTTP client step で実装")

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        raise NotImplementedError("Phase 8 後半 WS step で実装")

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("Phase 8 後半 WS step で実装")

    def events(self) -> AsyncIterator[LiveEvent]:
        raise NotImplementedError("Phase 8 後半 WS step で実装")
