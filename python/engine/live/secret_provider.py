"""SecondSecretResolver — Phase 9 Step 5 第二暗証番号の都度収集仲介。

``SecretVault`` (メモリ保管 + 60s TTL) と "UI に SecretRequired を push する
transport コールバック" を束ね、Tachibana ExecutionClient (adapter) が
``await resolve(venue, purpose)`` の 1 呼び出しで第二暗証番号を得られるようにする。

設計 (計画 §1.3 / §3.3.1):
1. ``vault.get(venue, purpose)`` で TTL 内の既存 secret を探す → あれば即返却
   (連続発注では SecretRequired を再発行しない)。
2. 無ければ ``vault.create_request`` で request_id を発行し、push コールバックで
   ``SecretRequired{request_id, venue, kind="second_secret", purpose}`` を UI に送る。
3. ``vault.wait_for`` で SubmitSecret RPC の到着を await (既定 30s)。timeout は
   ``SecretTimeoutError("SECRET_TIMEOUT")`` に変換する。

**transport 非依存**: 本モジュールは proto / BackendEventBus を import しない。
push コールバックは server_grpc が ``publish_backend_event`` に束ねて注入する
(secret_vault / order_facade と同じ reducer_bridge 思想)。平文 secret は
SecretVault の ``_store`` のみが保持し TTL で消える。resolver は値を保持しない。
"""
from __future__ import annotations

import asyncio
from typing import Callable, Protocol


class _SecretVault(Protocol):
    """SecondSecretResolver が依存する SecretVault の最小インタフェース。"""

    def get(self, venue: str, purpose: str) -> str | None: ...
    def create_request(self, venue: str, purpose: str) -> str: ...
    async def wait_for(self, request_id: str, timeout: float = ...) -> str: ...


# push(request_id, venue, kind, purpose) -> None
PushSecretRequired = Callable[[str, str, str, str], None]


class SecretTimeoutError(Exception):
    """UI から第二暗証番号の応答が来なかった (gRPC Res の error_code に載せる)。"""

    def __init__(self, error_code: str = "SECRET_TIMEOUT") -> None:
        super().__init__(error_code)
        self.error_code = error_code


class SecondSecretResolver:
    def __init__(
        self,
        vault: _SecretVault,
        push_secret_required: PushSecretRequired,
        *,
        timeout: float = 30.0,
    ) -> None:
        self._vault = vault
        self._push = push_secret_required
        self._timeout = timeout

    async def resolve(self, venue: str, purpose: str) -> str:
        """venue/purpose の第二暗証番号を取得する (cache → push → wait)。"""
        cached = self._vault.get(venue, purpose)
        if cached is not None:
            return cached

        request_id = self._vault.create_request(venue, purpose)
        # SecretRequired を UI に通知してから wait_for に入る。submit が push と
        # wait_for の間に届いても SecretVault が _store から後勝ちで拾うため安全。
        self._push(request_id, venue, "second_secret", purpose)
        try:
            return await self._vault.wait_for(request_id, timeout=self._timeout)
        except asyncio.TimeoutError as exc:
            raise SecretTimeoutError("SECRET_TIMEOUT") from exc
