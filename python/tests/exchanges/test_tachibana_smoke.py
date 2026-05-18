"""Smoke test: Tachibana demo の実 API に対する login + fetch_instruments (slow).

Phase 8 §3.2 D1。実 venue (demo 環境) に対して 1 往復だけ叩いて、
URL builder / auth.login / fetch_instruments のパイプライン全体を smoke する。

Marked @pytest.mark.slow — CI からは `-m 'not slow'` で除外。
`DEV_TACHIBANA_USER_ID` / `DEV_TACHIBANA_PASSWORD` のどちらかが未設定なら
自動 skip (handoff §"smoke test" 規約)。
"""
from __future__ import annotations

import os

import pytest

from engine.exchanges.tachibana import TachibanaAdapter
from engine.live.adapter import VenueCredentials

_ENV_USER_ID = "DEV_TACHIBANA_USER_ID"
_ENV_PASSWORD = "DEV_TACHIBANA_PASSWORD"

_CREDS_AVAILABLE = bool(os.environ.get(_ENV_USER_ID)) and bool(
    os.environ.get(_ENV_PASSWORD)
)


@pytest.mark.slow
@pytest.mark.skipif(
    not _CREDS_AVAILABLE,
    reason=f"requires {_ENV_USER_ID} and {_ENV_PASSWORD} (demo creds)",
)
async def test_tachibana_demo_login_and_fetch_instruments_smoke():
    """demo 環境で login → fetch_instruments → logout を 1 往復通す。

    Acceptance:
      - login() が例外を投げない (session 確立)
      - fetch_instruments() が空でない (CLMEventDownload が master を返す)
      - logout() で後始末
    """
    adapter = TachibanaAdapter(environment="demo")
    creds = VenueCredentials(credentials_source="env", environment_hint="demo")
    await adapter.login(creds)
    try:
        instruments = await adapter.fetch_instruments()
        assert isinstance(instruments, list)
        assert len(instruments) > 0, "demo master should return at least 1 instrument"
    finally:
        await adapter.logout()
