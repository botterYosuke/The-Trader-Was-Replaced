"""AccountSync spec (Phase 9 Step 4 — 口座同期 push)。

AccountSync は transport 非依存（proto を import しない。reducer_bridge と同思想）。
責務:
- 起動直後に 1 回 fetch_account して必ず emit（初期ロード = §3.12 / Success Criteria
  「起動時に余力・ポジションが表示される」を push で満たす）。
- 以降は interval_s 毎に fetch し、前回 emit した snapshot と異なるときだけ emit
  （等価判定は pydantic frozen の ==、ts_ms を持たない AccountSnapshot で時刻差を排除）。
- fetch_account の例外は warning ログ + last_error 記録して継続（best-effort・継続性優先）。
- stop() で task を cancel→await（CancelledError 握り）。
"""
from __future__ import annotations

import asyncio

import pytest

from engine.live.account_sync import AccountSync
from engine.live.adapter import VenueCredentials
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.order_types import AccountPositionData, AccountSnapshot


async def _logged_in_adapter() -> MockVenueAdapter:
    adapter = MockVenueAdapter()
    await adapter.login(
        VenueCredentials(credentials_source="env", environment_hint="demo")
    )
    return adapter


def test_emits_initial_snapshot_once_on_start() -> None:
    """起動直後に初期 snapshot が 1 回 emit される（初期ロードを push で満たす）。"""

    async def scenario() -> list[AccountSnapshot]:
        adapter = await _logged_in_adapter()
        adapter.set_account_snapshot(
            cash=1000.0,
            buying_power=2000.0,
            positions=[AccountPositionData(symbol="7203.TSE", qty=100, avg_price=2500.0, unrealized_pnl=10.0)],
        )
        seen: list[AccountSnapshot] = []
        sync = AccountSync(adapter, on_account_event=seen.append, interval_s=0.01)
        await sync.start()
        # 初回 emit を待つ（interval 前に必ず出るはず）
        for _ in range(20):
            if seen:
                break
            await asyncio.sleep(0.005)
        await sync.stop()
        return seen

    seen = asyncio.run(scenario())
    assert len(seen) >= 1
    first = seen[0]
    assert first.cash == 1000.0
    assert first.buying_power == 2000.0
    assert len(first.positions) == 1
    assert first.positions[0].symbol == "7203.TSE"


def test_unchanged_snapshot_not_re_emitted_changed_is() -> None:
    """snapshot 不変なら次 interval で emit されない。変化したら emit される。"""

    async def scenario() -> list[AccountSnapshot]:
        adapter = await _logged_in_adapter()
        adapter.set_account_snapshot(cash=100.0, buying_power=200.0, positions=[])
        seen: list[AccountSnapshot] = []
        sync = AccountSync(adapter, on_account_event=seen.append, interval_s=0.01)
        await sync.start()
        # 初回 emit を待つ
        for _ in range(20):
            if len(seen) >= 1:
                break
            await asyncio.sleep(0.005)
        # 数 interval 待っても不変なので 1 件のまま
        await asyncio.sleep(0.06)
        count_after_idle = len(seen)
        # snapshot を変えると次 interval で再 emit
        adapter.set_account_snapshot(cash=150.0, buying_power=200.0, positions=[])
        for _ in range(40):
            if len(seen) > count_after_idle:
                break
            await asyncio.sleep(0.005)
        await sync.stop()
        return seen

    seen = asyncio.run(scenario())
    # 不変中は 1 件のまま（初回だけ）
    assert seen[0].cash == 100.0
    # 変化後に新しい snapshot が来る
    cashes = [s.cash for s in seen]
    assert 150.0 in cashes
    # 同一 snapshot の連続重複は無い（隣接が必ず != ）
    for a, b in zip(seen, seen[1:]):
        assert a != b


def test_stop_terminates_task_cleanly() -> None:
    """stop() で task が綺麗に終わる（CancelledError を外へ漏らさない）。"""

    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        adapter.set_account_snapshot(cash=1.0, buying_power=1.0, positions=[])
        sync = AccountSync(adapter, on_account_event=lambda _s: None, interval_s=0.01)
        await sync.start()
        await asyncio.sleep(0.02)
        await asyncio.wait_for(sync.stop(), timeout=1.0)
        # 二重 stop は no-op
        await asyncio.wait_for(sync.stop(), timeout=1.0)

    asyncio.run(scenario())


def test_fetch_exception_does_not_kill_loop_records_last_error() -> None:
    """fetch_account 例外時にループが死なず last_error に記録され、次回正常 snapshot で再 emit。"""

    class _FlakyAdapter(MockVenueAdapter):
        def __init__(self) -> None:
            super().__init__()
            self._boom_calls = 0

        async def fetch_account(self) -> AccountSnapshot:  # type: ignore[override]
            self._boom_calls += 1
            # 初回成功（初期 emit）、2 回目だけ例外、それ以降は仕込み snapshot を返す
            if self._boom_calls == 2:
                raise RuntimeError("transient venue error")
            return await super().fetch_account()

    async def scenario() -> tuple[list[AccountSnapshot], BaseException | None]:
        adapter = _FlakyAdapter()
        await adapter.login(
            VenueCredentials(credentials_source="env", environment_hint="demo")
        )
        adapter.set_account_snapshot(cash=10.0, buying_power=20.0, positions=[])
        seen: list[AccountSnapshot] = []
        sync = AccountSync(adapter, on_account_event=seen.append, interval_s=0.01)
        await sync.start()
        # 初回 emit
        for _ in range(20):
            if len(seen) >= 1:
                break
            await asyncio.sleep(0.005)
        # 2 回目の fetch が例外を起こすまで待つ（last_error 記録）
        for _ in range(40):
            if sync.last_error is not None:
                break
            await asyncio.sleep(0.005)
        captured = sync.last_error
        # 例外後もループ継続: snapshot を変えると再 emit される
        adapter.set_account_snapshot(cash=99.0, buying_power=20.0, positions=[])
        for _ in range(60):
            if any(s.cash == 99.0 for s in seen):
                break
            await asyncio.sleep(0.005)
        await sync.stop()
        return seen, captured

    seen, captured = asyncio.run(scenario())
    assert captured is not None
    assert isinstance(captured, RuntimeError)
    # ループが死んでいないことの証明: 例外後の新 snapshot が emit された
    assert any(s.cash == 99.0 for s in seen)


def test_callback_exception_does_not_kill_loop() -> None:
    """on_account_event 内の例外でループが死なない（try で囲みログのみ）。"""

    async def scenario() -> int:
        adapter = await _logged_in_adapter()
        adapter.set_account_snapshot(cash=1.0, buying_power=1.0, positions=[])
        calls = {"n": 0}

        def cb(_s: AccountSnapshot) -> None:
            calls["n"] += 1
            if calls["n"] == 1:
                raise RuntimeError("callback boom on first emit")

        sync = AccountSync(adapter, on_account_event=cb, interval_s=0.01)
        await sync.start()
        # 初回 callback が例外でもループは生きており、変化後の 2 回目 callback が走る
        for _ in range(20):
            if calls["n"] >= 1:
                break
            await asyncio.sleep(0.005)
        adapter.set_account_snapshot(cash=2.0, buying_power=1.0, positions=[])
        for _ in range(40):
            if calls["n"] >= 2:
                break
            await asyncio.sleep(0.005)
        await sync.stop()
        return calls["n"]

    n = asyncio.run(scenario())
    assert n >= 2  # 初回例外後も 2 回目 emit が来た


def test_fetch_exception_records_source_aware_error() -> None:
    """fetch 例外が source 付き error record として観測できる。"""

    class _FlakyAdapter(MockVenueAdapter):
        def __init__(self) -> None:
            super().__init__()
            self._boom_calls = 0

        async def fetch_account(self) -> AccountSnapshot:  # type: ignore[override]
            self._boom_calls += 1
            if self._boom_calls == 2:
                raise RuntimeError("transient venue error")
            return await super().fetch_account()

    async def scenario() -> "AccountSync":
        adapter = _FlakyAdapter()
        await adapter.login(
            VenueCredentials(credentials_source="env", environment_hint="demo")
        )
        adapter.set_account_snapshot(cash=10.0, buying_power=20.0, positions=[])
        seen: list[AccountSnapshot] = []
        sync = AccountSync(adapter, on_account_event=seen.append, interval_s=0.01)
        await sync.start()
        for _ in range(40):
            if sync.last_error is not None:
                break
            await asyncio.sleep(0.005)
        await sync.stop()
        return sync

    sync = asyncio.run(scenario())
    rec = sync.last_error_record
    assert rec is not None
    assert rec.source == "account_sync"
    assert "transient venue error" in rec.detail


def test_force_resync_emits_even_for_unchanged_snapshot() -> None:
    """force-resync は dedup を貫通し、前回 emit と同一 snapshot でも再 emit する。

    issue #29 Slice 2': Replay→Live 切替直後に Rust が PortfolioState を reset した後、
    backend へ強制 snapshot を要求する経路の土台。値不変の demo 口座でも必ず再 push する。
    """

    async def scenario() -> list[AccountSnapshot]:
        adapter = await _logged_in_adapter()
        adapter.set_account_snapshot(cash=100.0, buying_power=200.0, positions=[])
        seen: list[AccountSnapshot] = []
        # interval を長くして「初期 emit のみ」の状態を作る（tick による再 emit を排除）。
        sync = AccountSync(adapter, on_account_event=seen.append, interval_s=1000.0)
        await sync.start()
        # 初回 emit を待つ
        for _ in range(40):
            if len(seen) >= 1:
                break
            await asyncio.sleep(0.005)
        assert len(seen) == 1
        # snapshot は不変のまま force-resync を要求 → dedup を貫通して再 emit されるはず
        await sync.force_resync()
        await sync.stop()
        return seen

    seen = asyncio.run(scenario())
    assert len(seen) == 2, "force-resync は同一 snapshot でも再 emit しなければならない"
    assert seen[0] == seen[1]
    assert seen[1].cash == 100.0
