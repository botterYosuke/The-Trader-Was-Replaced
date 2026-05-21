"""VenueHealthWatchdog spec (Phase 9 Step 7 — venue 本体ログアウト検知, §3.5)。

VenueHealthWatchdog は transport / venue 非依存。adapter.check_health() を interval 毎に
ポーリングし、False (本体ログアウト) を返したら on_venue_logout(venue) を 1 回だけ呼ぶ。
- 健康 (True) のときは何も通知しない。
- ログアウト検出は debounce: 復旧 (True) を観測するまで再通知しない。
- 復旧後にまたログアウトしたら再通知する (re-arm)。
- check_health の例外は warning + last_error 記録でループ継続 (誤って modal を出さない)。
- stop() で task を cancel→await (CancelledError 握り)。
"""
from __future__ import annotations

import asyncio
from typing import Optional

from engine.live.health_watchdog import VenueHealthWatchdog


class _FakeHealthSource:
    """check_health() の返り値を test がフレーム毎に差し替えられる fake adapter。"""

    def __init__(self) -> None:
        # None=raise(transient) / True=healthy / False=logged out
        self.next_health: object = True
        self.calls = 0

    async def check_health(self) -> bool:
        self.calls += 1
        if self.next_health is None:
            raise RuntimeError("transient venue error")
        return bool(self.next_health)


async def _wait(pred, *, tries: int = 60, step: float = 0.005) -> None:
    for _ in range(tries):
        if pred():
            return
        await asyncio.sleep(step)


def test_healthy_never_notifies() -> None:
    """check_health が True を返し続ける限り on_venue_logout は呼ばれない。"""

    async def scenario() -> list[str]:
        src = _FakeHealthSource()
        src.next_health = True
        fired: list[str] = []
        wd = VenueHealthWatchdog(
            src, venue_id="KABU", on_venue_logout=fired.append, interval_s=0.01
        )
        await wd.start()
        await _wait(lambda: src.calls >= 3)
        await wd.stop()
        return fired

    assert asyncio.run(scenario()) == []


def test_logout_notifies_once_then_debounced() -> None:
    """ログアウト検出は 1 回だけ通知し、復旧前は連打しない。"""

    async def scenario() -> list[str]:
        src = _FakeHealthSource()
        src.next_health = False  # ずっとログアウト
        fired: list[str] = []
        wd = VenueHealthWatchdog(
            src, venue_id="KABU", on_venue_logout=fired.append, interval_s=0.01
        )
        await wd.start()
        # 複数 tick 回しても通知は 1 回 (debounce)
        await _wait(lambda: src.calls >= 4)
        await wd.stop()
        return fired

    fired = asyncio.run(scenario())
    assert fired == ["KABU"]


def test_recovery_rearms_for_next_logout() -> None:
    """復旧 (True) を観測すると debounce が解除され、次のログアウトで再通知される。"""

    async def scenario() -> list[str]:
        src = _FakeHealthSource()
        src.next_health = False
        fired: list[str] = []
        wd = VenueHealthWatchdog(
            src, venue_id="KABU", on_venue_logout=fired.append, interval_s=0.01
        )
        await wd.start()
        await _wait(lambda: len(fired) == 1)  # 1 回目のログアウト通知
        src.next_health = True  # 復旧 → re-arm
        calls_at_recovery = src.calls
        await _wait(lambda: src.calls >= calls_at_recovery + 2)
        src.next_health = False  # 再ログアウト
        await _wait(lambda: len(fired) == 2)
        await wd.stop()
        return fired

    fired = asyncio.run(scenario())
    assert fired == ["KABU", "KABU"]


def test_transient_exception_does_not_notify_and_records_last_error() -> None:
    """check_health の例外では通知せず last_error に記録しループ継続する。"""

    async def scenario() -> tuple[list[str], Optional[BaseException]]:
        src = _FakeHealthSource()
        src.next_health = None  # raise transient
        fired: list[str] = []
        wd = VenueHealthWatchdog(
            src, venue_id="KABU", on_venue_logout=fired.append, interval_s=0.01
        )
        await wd.start()
        await _wait(lambda: wd.last_error is not None)
        captured = wd.last_error
        # 例外後もループ継続: 健康に戻ると通知無し / その後ログアウトで通知される
        src.next_health = True
        await _wait(lambda: src.calls >= 5)
        src.next_health = False
        await _wait(lambda: len(fired) == 1)
        await wd.stop()
        return fired, captured

    fired, captured = asyncio.run(scenario())
    assert isinstance(captured, RuntimeError)
    assert fired == ["KABU"]  # 例外では誤って firing せず、本物のログアウトでのみ通知


def test_callback_exception_retries_next_tick() -> None:
    """on_venue_logout が例外でも emit 済みにせず、次 tick で再通知を試みる。"""

    async def scenario() -> int:
        src = _FakeHealthSource()
        src.next_health = False
        calls = {"n": 0}

        def cb(_v: str) -> None:
            calls["n"] += 1
            if calls["n"] == 1:
                raise RuntimeError("callback boom on first notify")

        wd = VenueHealthWatchdog(
            src, venue_id="KABU", on_venue_logout=cb, interval_s=0.01
        )
        await wd.start()
        await _wait(lambda: calls["n"] >= 2)
        await wd.stop()
        return calls["n"]

    # 初回 callback 例外 → 「emit 済み」にしない → 次 tick で再通知される
    assert asyncio.run(scenario()) >= 2


def test_stop_is_idempotent() -> None:
    """stop() は CancelledError を漏らさず、二重呼び出しでも安全。"""

    async def scenario() -> None:
        src = _FakeHealthSource()
        wd = VenueHealthWatchdog(
            src, venue_id="KABU", on_venue_logout=lambda _v: None, interval_s=0.01
        )
        await wd.start()
        await asyncio.sleep(0.02)
        await asyncio.wait_for(wd.stop(), timeout=1.0)
        await asyncio.wait_for(wd.stop(), timeout=1.0)

    asyncio.run(scenario())
