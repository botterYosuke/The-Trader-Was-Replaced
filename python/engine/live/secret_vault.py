"""SecretVault — Tachibana 専用 secret 仲介（Phase 9 §3.1 / §3.2）。

責務: フロント(Rust)が後から渡す secret を、login/order RPC ハンドラが
await で受け取るための一時保管所。

公開 API:
- create_request(venue, purpose) -> request_id : UUID 発行 + pending Future 登録
- await wait_for(request_id, timeout=30.0) -> secret : Future を await
- submit(request_id, secret) -> None : Future.set_result + store(TTL)
- get(venue, purpose) -> str | None : TTL 内 secret（無ければ None）

設計メモ:
- create_request / submit / get は同期メソッド。wait_for のみ async。
- secret は store[(venue, purpose)] に保管し、submit 時刻起点で ttl 秒後に
  call_later で 1 回だけ削除する（再利用ではリセットしない）。purpose 別独立。
- event loop は遅延取得。create_request / submit は running loop が無くても
  壊れない（Future は明示 loop 無しで生成、call_later は loop があれば仕掛ける）。
- セキュリティ(§1.3): secret / Future の中身を repr・ログに晒さない。
- ライフサイクル(§1.3): 平文 secret は _store のみが保持し TTL で確実に削除する（漏洩面）。
  submit が wait_for より先行した取りこぼしも _store 逆引きで解決し、平文を別 dict に
  二重保持しない（§6: pickle に平文を残さない）。_pending は wait_for の timeout 時に
  掃除する。request_id 単位の _targets/_ttl_armed はメタデータのみで平文を含まず、
  Phase 9 scope では掃除せず据え置く（1 セッションの発注回数程度の str 蓄積で
  漏洩面ではない）。無制限増加が問題化する運用が出たら _expire での request_id
  逆引き掃除を後続 Phase で追加する。
"""
from __future__ import annotations

import asyncio
import threading
import uuid

DEFAULT_TTL = 60.0


class SecretVault:
    def __init__(self, ttl: float = DEFAULT_TTL) -> None:
        self._ttl = ttl
        # request_id -> pending Future（wait_for が running loop 上で遅延生成、submit で resolve）
        self._pending: dict[str, asyncio.Future[str]] = {}
        # request_id -> (venue, purpose)（submit 時の逆引き / wait_for 後勝ち時の _store 逆引き用）
        self._targets: dict[str, tuple[str, str]] = {}
        # (venue, purpose) -> secret（TTL 内のみ存在）
        self._store: dict[tuple[str, str], str] = {}
        # dict 操作の最小区間だけ保護（lock 内で set_result / await はしない）
        self._lock = threading.Lock()
        # 初回 submit で TTL call_later を仕掛けた request_id（再 arm 抑止）
        self._ttl_armed: set[str] = set()

    def create_request(self, venue: str, purpose: str) -> str:
        request_id = str(uuid.uuid4())
        # Future はここで作らない（loop に一切触れない）。running loop の無い
        # main thread から呼ばれ、閉じた loop に bind した Future を握ると submit 時に
        # "Event loop is closed" になるため。Future は wait_for が running loop 上で遅延生成する。
        with self._lock:
            self._targets[request_id] = (venue, purpose)
        return request_id

    async def wait_for(self, request_id: str, timeout: float = 30.0) -> str:
        loop = asyncio.get_running_loop()
        with self._lock:
            target = self._targets.get(request_id)
            if target is None:
                raise KeyError("unknown request_id")
            # submit が先行していれば TTL 管理下の _store から解決値を即返す（取りこぼし防止）。
            # 平文は _store のみが保持し TTL で確実に消えるため、別 dict に平文を二重保持しない(§1.3/§6)。
            cached = self._store.get(target)
            if cached is not None:
                return cached
            # まだ submit 前: running loop 上で Future を生成して登録（loop に正しく bind）。
            future: asyncio.Future[str] = loop.create_future()
            self._pending[request_id] = future
        try:
            return await asyncio.wait_for(future, timeout=timeout)
        finally:
            # timeout 中断時に _pending が漏れないよう必ず掃除。
            # submit 成功時は submit 側が pop 済みなので no-op（default None で二重 pop 安全）。
            with self._lock:
                self._pending.pop(request_id, None)

    def submit(self, request_id: str, secret: str) -> None:
        # dict 操作のみ lock 内。Future 解決 / call_later は loop thread に委譲する。
        with self._lock:
            target = self._targets.get(request_id)
            if target is None:
                raise KeyError("unknown request_id")
            self._store[target] = secret
            future = self._pending.pop(request_id, None)
            # wait_for より先行した submit は _store に保管済み。wait_for が後勝ちで _store から拾う。
            # 初回 submit のみ TTL を arm（2 回目以降は失効時刻を延ばさない）。
            arm_ttl = request_id not in self._ttl_armed
            if arm_ttl:
                self._ttl_armed.add(request_id)

        # Future が live loop に bind されていれば、その loop を threadsafe に起こす。
        # Future はすべて wait_for が running loop 上で生成済みなので get_loop() は安全。
        loop = None
        if future is not None and not future.done():
            loop = future.get_loop()
            loop.call_soon_threadsafe(future.set_result, secret)

        if not arm_ttl:
            return
        # TTL: call_later は非 threadsafe なので loop thread 上に仕掛ける。
        if loop is None:
            try:
                loop = asyncio.get_running_loop()
            except RuntimeError:
                loop = None
        if loop is not None:
            loop.call_soon_threadsafe(loop.call_later, self._ttl, self._expire, target)

    def get(self, venue: str, purpose: str) -> str | None:
        with self._lock:
            return self._store.get((venue, purpose))

    def _expire(self, target: tuple[str, str]) -> None:
        with self._lock:
            self._store.pop(target, None)

    def __repr__(self) -> str:
        # secret / Future の中身は晒さない。件数のみ。
        return (
            f"SecretVault(pending={len(self._pending)}, "
            f"stored={len(self._store)}, ttl={self._ttl})"
        )
