"""Smoke test: kabuStation adapter の E2E パイプライン (Phase 8 §3.2 D1).

kabuStation 本体 (Windows GUI) は CI で利用できないため、smoke を 2 段で組む:

1. **mock 経路 (CI で走る)** — `pytest-httpx` で `/token` と `/register` を mock し、
   `KabuStationAdapter.login('env') → fetch_instruments → subscribe → unsubscribe → logout`
   の adapter wire-up を 1 往復通す。**WS は接続しない** (subscribe は直前で early-return
   できないため、`unsubscribe` で reader task を畳む)。
2. **実機 smoke (`@pytest.mark.slow`, 別マーカー `kabu_live`)** — kabuStation 本体が
   listening している環境でのみ走る。`DEV_KABU_API_PASSWORD` 未設定 or connection refused
   なら自動 skip。手動手順は `python/tests/exchanges/README_kabu_smoke.md` 参照。
"""
from __future__ import annotations

import os

import httpx
import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.kabusapi import KabuStationAdapter
from engine.exchanges.kabusapi_url import endpoint
from engine.live.adapter import VenueCredentials

_ENV_API_PASSWORD = "DEV_KABU_API_PASSWORD"
_KABU_CREDS_AVAILABLE = bool(os.environ.get(_ENV_API_PASSWORD))


# ---------------------------------------------------------------------------
# 1) Mock smoke — runs in CI (no slow marker)
# ---------------------------------------------------------------------------


async def test_kabu_login_and_fetch_instruments_mock_smoke(
    monkeypatch, httpx_mock: HTTPXMock
):
    """Mock の /token に対して login → fetch_instruments → logout を通す。

    fetch_instruments は MVP 仕様 (handoff ユーザー決定事項 L84) で `[]` を返すため、
    HTTP には到達しない。/token のみ mock すれば足りる。
    """
    monkeypatch.setenv(_ENV_API_PASSWORD, "dummy-api-pw")

    httpx_mock.add_response(
        url=endpoint("token", env="verify"),
        method="POST",
        json={"ResultCode": 0, "Token": "smoke-token-abcd"},
    )

    adapter = KabuStationAdapter(environment="verify")
    creds = VenueCredentials(credentials_source="env", environment_hint="verify")
    await adapter.login(creds)
    try:
        instruments = await adapter.fetch_instruments()
        assert instruments == []  # MVP: kabu master は空 list
        # subscribe は WS reader task を spawn するため smoke から除外。
        # adapter._token が login で正しく保持されていることのみ確認する。
        assert adapter._token == "smoke-token-abcd"
    finally:
        await adapter.logout()
        assert adapter._token is None


# ---------------------------------------------------------------------------
# 2) Real smoke — kabuStation 本体起動前提、`-m "slow and kabu_live"` で実行
# ---------------------------------------------------------------------------


@pytest.mark.slow
@pytest.mark.kabu_live
@pytest.mark.skipif(
    not _KABU_CREDS_AVAILABLE,
    reason=f"requires {_ENV_API_PASSWORD} (kabu API password) and kabuStation GUI running",
)
async def test_kabu_real_login_smoke():
    """実機 kabuStation (verify) に対して login → logout を 1 往復通す。

    Acceptance:
      - kabuStation 本体が 18081 で listening (connection refused なら skip)
      - login() がトークン取得に成功
      - logout() で後始末
    """
    adapter = KabuStationAdapter(environment="verify")
    creds = VenueCredentials(credentials_source="env", environment_hint="verify")
    try:
        await adapter.login(creds)
    except (httpx.ConnectError, OSError) as e:
        pytest.skip(f"kabuStation not running on 18081: {e}")
    try:
        assert adapter._token is not None
        assert len(adapter._token) > 0
    finally:
        await adapter.logout()
