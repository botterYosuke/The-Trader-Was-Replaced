"""InstrumentsScheduler — Phase 9 Step 9 の銘柄メタデータ日次更新。

責務（§3.6 / Success Criteria「営業日 5:00 JST に Instruments parquet が atomic 更新」）:
- 起動直後に 1 回 `fetch_instruments()` → persist（= **ログイン時 persist** / 初期ロード）。
- 以降は **次の 5:00 JST まで sleep** して再 fetch+persist を繰り返す。

設計判断:
- **営業日カレンダーを持たない**（ユーザー決定）。J-Quants `/markets/trading_calendar`
  クライアントは未実装で、build すると外部依存・credentials が増える。代わりに
  非営業日/閉局は **venue 側の `fetch_instruments()` がエラー or 空を返す**のに委ね、
  本スケジューラは「失敗 → 前回 parquet を保持してスキップ・翌 5:00 に再試行」する。
  → forward-compat: trading_calendar 連携は将来 `next_delay_s` / business-day gate の
    差し替えで足せる（計画書 §3.6 のドリフト訂正として記録）。
- **transport / store 非依存**（account_sync と同思想）。`persist` コールバックを注入で
  受け、既定は `instruments_store.write_instruments`。`next_delay_s` も注入可能にして
  5:00 JST の実待ちなしにループを検証できる。
- **resilience**: fetch / persist の例外は warning + last_error 記録してループ継続
  （account_sync と同じ best-effort）。`CancelledError` のみ正常終了。空リストは
  「adapter 非対応（kabu MVP）」として persist しない。
"""
from __future__ import annotations

import asyncio
import logging
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo
from typing import Callable, List, Optional, Protocol

from engine.live import instruments_store
from engine.live.adapter import InstrumentRaw

_LOG = logging.getLogger(__name__)
_JST = ZoneInfo("Asia/Tokyo")
_REFRESH_HOUR = 5


def seconds_until_next_5am_jst(now: datetime) -> float:
    """`now`（tz-aware）から次の 5:00 JST までの秒数。ちょうど 5:00 なら 24h 後。"""
    now_jst = now.astimezone(_JST)
    target = now_jst.replace(hour=_REFRESH_HOUR, minute=0, second=0, microsecond=0)
    if target <= now_jst:
        target += timedelta(days=1)
    return (target - now_jst).total_seconds()


class _InstrumentSource(Protocol):
    async def fetch_instruments(self) -> List[InstrumentRaw]: ...


class InstrumentsScheduler:
    """起動時 1 回 + 営業日 5:00 JST 毎に銘柄メタを fetch して parquet を atomic 更新。"""

    def __init__(
        self,
        adapter: _InstrumentSource,
        venue_id: str,
        *,
        persist: Optional[Callable[[str, List[InstrumentRaw]], object]] = None,
        next_delay_s: Optional[Callable[[], float]] = None,
        now_fn: Optional[Callable[[], datetime]] = None,
    ) -> None:
        self._adapter = adapter
        self._venue_id = venue_id
        self._persist = persist or instruments_store.write_instruments
        self._now_fn = now_fn or (lambda: datetime.now(timezone.utc))
        self._next_delay_s = next_delay_s or (
            lambda: seconds_until_next_5am_jst(self._now_fn())
        )
        self._task: Optional[asyncio.Task[None]] = None
        self._last_error: Optional[BaseException] = None
        # Issue #32 Slice 2: 初回 refresh（ログイン時 persist）が完了したか。
        # `is_warming()` がこれを参照し、cold-store の PENDING 判定に使う。
        self._initial_refresh_done = False

    async def start(self) -> None:
        if self._task is not None and not self._task.done():
            return
        self._last_error = None
        self._initial_refresh_done = False
        self._task = asyncio.create_task(self._run())

    def is_warming(self) -> bool:
        """初回 refresh（ログイン時 persist）が進行中なら True。

        起動前（task 未生成）と初回 refresh 完了後は False。Issue #32 Slice 2 で
        _backend_impl が cold-store miss を 60s blocking fetch せず `LIVE_UNIVERSE_PENDING`
        に倒す判定に使う（store が埋まる前の picker クリックを Loading spinner にする）。
        """
        if self._task is None:
            return False
        return not self._initial_refresh_done

    async def _run(self) -> None:
        # 初期ロード: login 直後に即 fetch+persist（必ず 1 回 = ログイン時 persist）。
        try:
            await self._refresh()
        finally:
            # 初回が成功・失敗・cancel いずれでも warming は終わる（無限 spinner を防ぐ）。
            self._initial_refresh_done = True
        while True:
            try:
                await asyncio.sleep(self._next_delay_s())
            except asyncio.CancelledError:
                return
            await self._refresh()

    async def _refresh(self) -> None:
        try:
            raws = await self._adapter.fetch_instruments()
        except asyncio.CancelledError:
            raise
        except BaseException as exc:  # noqa: BLE001 — 非営業日/閉局は前回 parquet を保持
            self._last_error = exc
            _LOG.warning(
                "InstrumentsScheduler[%s]: fetch_instruments failed, keeping previous parquet",
                self._venue_id,
                exc_info=exc,
            )
            return

        if not raws:
            # 空 == adapter 非対応（kabu MVP は [] を返す）→ 既存 parquet を潰さない。
            _LOG.debug(
                "InstrumentsScheduler[%s]: empty instrument list, skip persist", self._venue_id
            )
            return

        try:
            self._persist(self._venue_id, raws)
        except asyncio.CancelledError:
            raise
        except BaseException as exc:  # noqa: BLE001 — persist 失敗でループを止めない
            self._last_error = exc
            _LOG.warning(
                "InstrumentsScheduler[%s]: persist failed", self._venue_id, exc_info=exc
            )

    async def stop(self) -> None:
        if self._task is None:
            return
        self._task.cancel()
        try:
            await self._task
        except asyncio.CancelledError:
            pass
        self._task = None

    @property
    def last_error(self) -> Optional[BaseException]:
        return self._last_error
