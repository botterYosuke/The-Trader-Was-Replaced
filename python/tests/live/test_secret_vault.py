"""SecretVault spec (Phase 9 §3.1 / §3.2 — Tachibana 専用 secret 仲介)。

責務: フロント(Rust)が後から渡す secret を、login/order RPC ハンドラが
await で受け取るための一時保管所。
- create_request(venue, purpose) -> request_id  : UUID 発行 + pending Future 登録
- await wait_for(request_id, timeout=30.0) -> secret : Future を await
- submit(request_id, secret) -> None             : Future.set_result + store(TTL)
- get(venue, purpose) -> str | None              : TTL 内 secret（無ければ None）

TTL の経過/並行/timeout の本格 spec は S3 に回す。S1 は core happy-path のみ。
"""
from __future__ import annotations

import asyncio
import threading
import time

from engine.live.secret_vault import SecretVault


def test_create_request_returns_unique_ids():
    async def scenario():
        vault = SecretVault()
        ids = [
            vault.create_request("tachibana", "login")
            for _ in range(5)
        ]
        return ids

    ids = asyncio.run(scenario())
    assert len(set(ids)) == 5
    assert all(isinstance(i, str) and i for i in ids)


def test_get_returns_none_before_submit():
    async def scenario():
        vault = SecretVault()
        vault.create_request("tachibana", "login")
        return vault.get("tachibana", "login")

    assert asyncio.run(scenario()) is None


def test_get_returns_secret_after_submit():
    async def scenario():
        vault = SecretVault()
        rid = vault.create_request("tachibana", "login")
        vault.submit(rid, "hunter2")
        return vault.get("tachibana", "login")

    assert asyncio.run(scenario()) == "hunter2"


def test_wait_for_resolves_after_submit():
    async def scenario():
        vault = SecretVault()
        rid = vault.create_request("tachibana", "order")

        async def submitter():
            await asyncio.sleep(0)
            vault.submit(rid, "topsecret")

        asyncio.create_task(submitter())
        return await vault.wait_for(rid, timeout=1.0)

    assert asyncio.run(scenario()) == "topsecret"


def test_get_returns_none_after_ttl_expires():
    async def scenario():
        vault = SecretVault(ttl=0.05)
        rid = vault.create_request("tachibana", "login")
        vault.submit(rid, "expiring")
        before = vault.get("tachibana", "login")
        await asyncio.sleep(0.1)
        after = vault.get("tachibana", "login")
        return before, after

    before, after = asyncio.run(scenario())
    assert before == "expiring"
    assert after is None


def test_reuse_does_not_extend_ttl():
    async def scenario():
        vault = SecretVault(ttl=0.3)
        rid = vault.create_request("tachibana", "login")
        vault.submit(rid, "expiring")
        # 途中で reuse（get）しても失効時刻は submit 起点のまま動かない
        await asyncio.sleep(0.1)
        midway = vault.get("tachibana", "login")
        await asyncio.sleep(0.4)
        after = vault.get("tachibana", "login")
        return midway, after

    midway, after = asyncio.run(scenario())
    assert midway == "expiring"
    assert after is None


def test_wait_for_times_out_when_no_submit():
    async def scenario():
        vault = SecretVault()
        rid = vault.create_request("tachibana", "order")
        try:
            await vault.wait_for(rid, timeout=0.05)
        except asyncio.TimeoutError:
            return "timeout"
        return "resolved"

    assert asyncio.run(scenario()) == "timeout"


def test_wait_for_unknown_request_id_raises_keyerror():
    async def scenario():
        vault = SecretVault()
        try:
            await vault.wait_for("does-not-exist", timeout=0.05)
        except KeyError:
            return "keyerror"
        return "no-error"

    assert asyncio.run(scenario()) == "keyerror"


def test_submit_from_worker_thread_resolves_wait_for_on_loop():
    """SubmitSecret RPC handler は sync ThreadPool の worker thread から submit する。
    一方 wait_for は live loop thread で await される（server_grpc の
    run_coroutine_threadsafe 経路）。plain future.set_result は loop を起こさず
    dead-wait になるため、cross-thread submit でも wait_for が解けることを固定する。
    """
    async def scenario():
        vault = SecretVault()
        rid = vault.create_request("tachibana", "login")

        def worker():
            time.sleep(0.02)
            vault.submit(rid, "from-worker")

        threading.Thread(target=worker, daemon=True).start()
        started = time.monotonic()
        # cross-thread submit が loop を起こせば ~0.02s で解ける。
        # plain set_result（dead-wait）なら timeout=2.0 起床まで待たされる。
        secret = await vault.wait_for(rid, timeout=2.0)
        elapsed = time.monotonic() - started
        return secret, elapsed

    secret, elapsed = asyncio.run(scenario())
    assert secret == "from-worker"
    # loop を threadsafe に起こさないと elapsed が timeout 近くまで膨らむ。
    assert elapsed < 0.5, f"cross-thread submit did not wake loop promptly: {elapsed:.3f}s"


def test_double_submit_keeps_first_ttl_and_does_not_raise():
    """2 回目 submit は KeyError にならず（Future は pop 済み）、TTL は初回起点を維持。
    2 回目で新たな call_later を仕掛けて失効を遅らせないこと。

    タイミング設計（フルスイート高負荷でも安定するよう全マージンを十分広く取る）:
      ttl=0.4。初回 submit を t=0、2 回目 submit を t=0.2、最終 get を t=0.8 で行う。
      正しい挙動 = 初回起点で失効: 失効時刻 t=0.4。最終 get(t=0.8) は失効後 0.4s 余裕。
      もし 2 回目で TTL を再 arm したら失効時刻は t=0.2+0.4=0.6 にズレるが、それでも
      最終 get(t=0.8) は失効後 0.2s なので "None" になり、バグを検出できない…という
      退行を避けるため、最終 get を「初回起点失効(0.4)後・誤再 arm 失効(0.6)前」では
      なく『誤再 arm でも失効する点(t=0.8 > 0.6)』に置く戦略は採らない。
      代わりに get 時刻 t=0.55 を採用: 正しい初回起点失効(0.4)後だが、誤再 arm 失効
      (0.6)より前。よって「初回 TTL 維持なら None / 誤って延長したら "second"」を
      確実に判別でき、各間隔(0.2 / 0.15 / 0.15)が高負荷スリップにも耐える。
    """
    async def scenario():
        vault = SecretVault(ttl=0.4)
        rid = vault.create_request("tachibana", "login")
        vault.submit(rid, "first")        # t=0   初回 submit → 失効予定 t=0.4
        await asyncio.sleep(0.2)
        vault.submit(rid, "second")       # t=0.2 初回 TTL を延ばしてはいけない（延ばすと失効 t=0.6）
        await asyncio.sleep(0.35)         # t=0.55: 初回起点(0.4)後・誤再 arm(0.6)前
        return vault.get("tachibana", "login")

    # 初回 TTL を維持していれば失効済み → None。誤って延長していれば "second" が残る。
    assert asyncio.run(scenario()) is None


def test_wait_for_picks_up_secret_when_submit_precedes():
    """submit が wait_for より先行しても、後から来た wait_for が _store から後勝ちで拾う。
    平文は _store のみが保持し（TTL 管理）、解決値を別 dict に二重保持しない(§1.3/§6)。
    """
    async def scenario():
        vault = SecretVault()
        rid = vault.create_request("tachibana", "login")
        vault.submit(rid, "early")  # wait_for より先行
        resolved = await vault.wait_for(rid, timeout=1.0)
        # 解決後に平文を保持する request_id 単位の dict が無いこと（pickle 平文残留防止）。
        leaked = [
            v for d in vars(vault).values()
            if isinstance(d, dict)
            for v in d.values()
            if v == "early"
        ]
        return resolved, leaked

    resolved, leaked = asyncio.run(scenario())
    assert resolved == "early"
    # _store[(v,p)] の 1 件のみ（TTL で消える）。request_id キーの平文保持は無いこと。
    assert leaked == ["early"], f"plaintext leaked into extra dict(s): {leaked}"
