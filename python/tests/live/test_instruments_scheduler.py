"""InstrumentsScheduler spec (Phase 9 Step 9 — 銘柄メタデータ日次更新)。

設計（§3.6 / account_sync と同思想・transport 非依存）:
- 起動直後に 1 回 fetch_instruments → persist（= **ログイン時 persist**。初期ロード）。
- 以降は **次の 5:00 JST まで sleep** して再 fetch+persist（営業日カレンダーは持たず、
  非営業日は venue が fetch_instruments でエラー/空を返すのに委ねる — ユーザー決定）。
- fetch_instruments の例外は warning + last_error 記録して **前回 parquet を保持し継続**
  （best-effort・account_sync と同じ resilience）。空リストは「adapter 非対応」で persist しない。
- stop() で task を cancel→await（CancelledError 握り）。

`seconds_until_next_5am_jst` は純関数として直接検証する（5:00 前/後/丁度）。
スケジューラ本体は `next_delay_s` を注入して高速ループで検証（5 JST の実待ちはしない）。
"""
from __future__ import annotations

import asyncio
from datetime import datetime, timezone
from zoneinfo import ZoneInfo

from engine.live.adapter import InstrumentRaw
from engine.live.instruments_scheduler import (
    InstrumentsScheduler,
    seconds_until_next_5am_jst,
)

_JST = ZoneInfo("Asia/Tokyo")


def _raws() -> list[InstrumentRaw]:
    return [InstrumentRaw(code="7203", name="トヨタ", market="TSE", tick_size=0.5, lot_size=100)]


class _FakeAdapter:
    def __init__(self, raws: list[InstrumentRaw]) -> None:
        self._raws = raws
        self.calls = 0

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        self.calls += 1
        return list(self._raws)


# --- seconds_until_next_5am_jst (pure) -------------------------------------


def test_seconds_until_5am_before_target_same_day() -> None:
    # 2026-05-21 03:00 JST → 同日 05:00 まで 2h
    now = datetime(2026, 5, 21, 3, 0, tzinfo=_JST)
    assert seconds_until_next_5am_jst(now) == 2 * 3600


def test_seconds_until_5am_after_target_next_day() -> None:
    # 2026-05-21 06:00 JST → 翌日 05:00 まで 23h
    now = datetime(2026, 5, 21, 6, 0, tzinfo=_JST)
    assert seconds_until_next_5am_jst(now) == 23 * 3600


def test_seconds_until_5am_exactly_target_rolls_to_next_day() -> None:
    now = datetime(2026, 5, 21, 5, 0, tzinfo=_JST)
    assert seconds_until_next_5am_jst(now) == 24 * 3600


def test_seconds_until_5am_accepts_utc_input() -> None:
    # 2026-05-20 19:00 UTC == 2026-05-21 04:00 JST → 1h まで
    now = datetime(2026, 5, 20, 19, 0, tzinfo=timezone.utc)
    assert seconds_until_next_5am_jst(now) == 3600


# --- scheduler lifecycle ----------------------------------------------------


def test_persists_on_start_login_time_persist() -> None:
    """起動直後に 1 回 fetch+persist する（ログイン時 persist = 初期ロード）。"""

    async def scenario() -> list[tuple[str, list[InstrumentRaw]]]:
        adapter = _FakeAdapter(_raws())
        persisted: list[tuple[str, list[InstrumentRaw]]] = []
        sched = InstrumentsScheduler(
            adapter,
            "TACHIBANA",
            persist=lambda v, raws: persisted.append((v, list(raws))),
            next_delay_s=lambda: 10.0,  # 初期 persist 後はすぐ起きない
        )
        await sched.start()
        for _ in range(40):
            if persisted:
                break
            await asyncio.sleep(0.005)
        await sched.stop()
        return persisted

    persisted = asyncio.run(scenario())
    assert len(persisted) == 1
    venue, raws = persisted[0]
    assert venue == "TACHIBANA"
    assert raws == _raws()


def test_periodic_refresh_fires_on_each_wakeup() -> None:
    """next_delay_s で起こすたびに再 fetch+persist する。"""

    async def scenario() -> int:
        adapter = _FakeAdapter(_raws())
        persisted: list[tuple[str, list[InstrumentRaw]]] = []
        sched = InstrumentsScheduler(
            adapter,
            "TACHIBANA",
            persist=lambda v, raws: persisted.append((v, list(raws))),
            next_delay_s=lambda: 0.01,
        )
        await sched.start()
        for _ in range(60):
            if len(persisted) >= 3:
                break
            await asyncio.sleep(0.005)
        await sched.stop()
        return len(persisted)

    assert asyncio.run(scenario()) >= 3


def test_empty_instruments_not_persisted() -> None:
    """空リスト（adapter 非対応 = kabu MVP）は persist しない。"""

    async def scenario() -> int:
        adapter = _FakeAdapter([])
        persisted: list = []
        sched = InstrumentsScheduler(
            adapter,
            "KABU",
            persist=lambda v, raws: persisted.append((v, raws)),
            next_delay_s=lambda: 0.01,
        )
        await sched.start()
        await asyncio.sleep(0.08)
        await sched.stop()
        # fetch は走るが persist は 0
        assert adapter.calls >= 1
        return len(persisted)

    assert asyncio.run(scenario()) == 0


def test_fetch_exception_keeps_previous_and_records_last_error() -> None:
    """非営業日/閉局で fetch_instruments が例外でもループは死なず、前回 persist を保持。"""

    class _FlakyAdapter:
        def __init__(self) -> None:
            self.calls = 0

        async def fetch_instruments(self) -> list[InstrumentRaw]:
            self.calls += 1
            if self.calls == 2:
                raise RuntimeError("market closed (holiday)")
            return _raws()

    async def scenario() -> tuple[int, BaseException | None]:
        adapter = _FlakyAdapter()
        persisted: list = []
        sched = InstrumentsScheduler(
            adapter,
            "TACHIBANA",
            persist=lambda v, raws: persisted.append((v, raws)),
            next_delay_s=lambda: 0.01,
        )
        await sched.start()
        for _ in range(60):
            if sched.last_error is not None and len(persisted) >= 2:
                break
            await asyncio.sleep(0.005)
        captured = sched.last_error
        await sched.stop()
        return len(persisted), captured

    n_persisted, captured = asyncio.run(scenario())
    assert isinstance(captured, RuntimeError)
    # 例外回はスキップされるが、その後の正常 fetch で persist が続く
    assert n_persisted >= 2


def test_stop_terminates_cleanly_and_double_stop_is_noop() -> None:
    async def scenario() -> None:
        adapter = _FakeAdapter(_raws())
        sched = InstrumentsScheduler(
            adapter, "TACHIBANA", persist=lambda v, raws: None, next_delay_s=lambda: 0.01
        )
        await sched.start()
        await asyncio.sleep(0.02)
        await asyncio.wait_for(sched.stop(), timeout=1.0)
        await asyncio.wait_for(sched.stop(), timeout=1.0)

    asyncio.run(scenario())


def test_default_persist_writes_real_parquet(monkeypatch, tmp_path) -> None:
    """persist 未指定なら instruments_store へ実 parquet を書く（フル結線の確認）。"""
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    from engine.live import instruments_store

    async def scenario() -> None:
        adapter = _FakeAdapter(_raws())
        sched = InstrumentsScheduler(adapter, "TACHIBANA", next_delay_s=lambda: 10.0)
        await sched.start()
        for _ in range(40):
            if (tmp_path / "tachibana.parquet").exists():
                break
            await asyncio.sleep(0.005)
        await sched.stop()

    asyncio.run(scenario())
    assert instruments_store.read_instruments("TACHIBANA") == _raws()


# --- warming state (Issue #32 Slice 2) -------------------------------------


def test_is_warming_true_during_initial_refresh_false_before_and_after() -> None:
    """Issue #32 Slice 2: 初回 refresh（ログイン時 persist）が進行中の間だけ
    `is_warming()` が True。起動前と初回完了後は False。

    この信号で _backend_impl は cold-store miss を 60s blocking fetch せず
    `LIVE_UNIVERSE_PENDING` に倒す（UI で Loading spinner にする）。"""

    async def scenario() -> None:
        gate = asyncio.Event()
        started = asyncio.Event()

        class _SlowAdapter:
            async def fetch_instruments(self) -> list[InstrumentRaw]:
                started.set()
                await gate.wait()  # 初回 refresh をテストが解放するまで止める
                return _raws()

        sched = InstrumentsScheduler(
            _SlowAdapter(),
            "TACHIBANA",
            persist=lambda v, raws: None,
            next_delay_s=lambda: 10.0,  # 初回後はすぐ起きない
        )
        assert sched.is_warming() is False, "起動前は warming ではない"

        await sched.start()
        await asyncio.wait_for(started.wait(), timeout=1.0)
        assert sched.is_warming() is True, "初回 refresh 進行中は warming"

        gate.set()  # 初回 refresh を完了させる
        for _ in range(100):
            if not sched.is_warming():
                break
            await asyncio.sleep(0.005)
        assert sched.is_warming() is False, "初回 refresh 完了後は warming ではない"

        await sched.stop()

    asyncio.run(scenario())
