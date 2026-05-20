"""Tests for SecondSecretResolver — Phase 9 Step 5 第二暗証番号の都度収集。

SecretVault と "SecretRequired を UI に push する transport コールバック" を束ね、
adapter から ``await resolve(venue, purpose)`` 1 発で第二暗証番号を取得できるように
する仲介層。transport 非依存 (proto を import しない): push コールバックは
server_grpc 側で publish_backend_event に束ねて注入する。
"""
from __future__ import annotations

import asyncio
import threading
import time

import pytest

from engine.live.secret_provider import SecondSecretResolver, SecretTimeoutError
from engine.live.secret_vault import SecretVault


def _resolver(vault, pushed):
    """push 履歴を記録する resolver を作る。"""
    def push(request_id, venue, kind, purpose):
        pushed.append((request_id, venue, kind, purpose))
    return SecondSecretResolver(vault, push)


def test_cache_hit_returns_without_push():
    """TTL 内に既存 secret があれば SecretRequired を push しない (連続発注 reuse)。"""
    async def scenario():
        vault = SecretVault()
        rid = vault.create_request("TACHIBANA", "new_order")
        vault.submit(rid, "cached")
        pushed: list = []
        secret = await _resolver(vault, pushed).resolve("TACHIBANA", "new_order")
        return secret, pushed

    secret, pushed = asyncio.run(scenario())
    assert secret == "cached"
    assert pushed == []  # cache hit → push なし


def test_miss_pushes_secret_required_then_resolves():
    """secret 不在 → SecretRequired を push し、SubmitSecret 後に解決する。"""
    async def scenario():
        vault = SecretVault()
        pushed: list = []
        resolver = _resolver(vault, pushed)

        async def submitter():
            await asyncio.sleep(0)
            # push された request_id に対して UI が応答する。
            rid = pushed[0][0]
            vault.submit(rid, "from-ui")

        asyncio.create_task(submitter())
        secret = await resolver.resolve("TACHIBANA", "cancel_order")
        return secret, pushed

    secret, pushed = asyncio.run(scenario())
    assert secret == "from-ui"
    assert len(pushed) == 1
    request_id, venue, kind, purpose = pushed[0]
    assert venue == "TACHIBANA"
    assert kind == "second_secret"
    assert purpose == "cancel_order"
    assert isinstance(request_id, str) and request_id


def test_second_resolve_reuses_cached_secret_no_push():
    """1 回 submit した後の 2 回目 resolve は cache hit で push しない。"""
    async def scenario():
        vault = SecretVault()
        pushed: list = []
        resolver = _resolver(vault, pushed)

        async def submitter():
            await asyncio.sleep(0)
            vault.submit(pushed[0][0], "once")

        asyncio.create_task(submitter())
        first = await resolver.resolve("TACHIBANA", "new_order")
        second = await resolver.resolve("TACHIBANA", "new_order")  # cache hit
        return first, second, pushed

    first, second, pushed = asyncio.run(scenario())
    assert first == "once"
    assert second == "once"
    assert len(pushed) == 1  # 2 回目は push されない


def test_timeout_raises_secret_timeout():
    """UI 応答が来ないと SECRET_TIMEOUT を上げる (永久 await しない)。"""
    async def scenario():
        vault = SecretVault()
        resolver = SecondSecretResolver(vault, lambda *a: None, timeout=0.05)
        try:
            await resolver.resolve("TACHIBANA", "new_order")
        except SecretTimeoutError as exc:
            return exc.error_code
        return "no-timeout"

    assert asyncio.run(scenario()) == "SECRET_TIMEOUT"


def test_resolve_works_when_submit_from_worker_thread():
    """SubmitSecret は gRPC worker thread から、resolve は live loop で走る。
    cross-thread submit でも resolve が速やかに解けること (SecretVault と同契約)。
    """
    async def scenario():
        vault = SecretVault()
        pushed: list = []
        resolver = _resolver(vault, pushed)

        def worker():
            # push が走るまで少し待ってから別スレッドで submit する。
            for _ in range(100):
                if pushed:
                    break
                time.sleep(0.005)
            vault.submit(pushed[0][0], "cross-thread")

        threading.Thread(target=worker, daemon=True).start()
        started = time.monotonic()
        secret = await resolver.resolve("TACHIBANA", "correct_order")
        return secret, time.monotonic() - started

    secret, elapsed = asyncio.run(scenario())
    assert secret == "cross-thread"
    assert elapsed < 1.0, f"cross-thread submit did not wake loop: {elapsed:.3f}s"
