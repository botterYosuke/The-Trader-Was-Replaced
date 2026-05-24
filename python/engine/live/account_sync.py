"""AccountSync — Phase 9 Step 4 の口座同期 push（余力・建玉の定期 fetch + 差分 emit）。

責務（§3.4 / Success Criteria「口座同期」）:
- 起動直後に 1 回 `fetch_account()` して **必ず emit**（初期ロード。GetAccount RPC を
  新設せず初回 push でまかなう — 計画書 §3.12 のドリフト訂正、下記設計判断参照）。
- 以降は `interval_s` 毎に fetch し、**前回 emit した snapshot と異なるときだけ emit**
  （等価判定は `AccountSnapshot` の pydantic frozen `==`。ts_ms を持たないため時刻差で
  誤判定しない）。

設計判断:
- **transport 非依存**: proto を import しない（reducer_bridge と同思想）。`on_account_event`
  コールバックに `AccountSnapshot` を渡すだけ。proto 変換と `ts_ms` 採番は server_grpc の責務。
- **callback は同期関数**: live loop thread 上で走り、server_grpc では threadsafe な
  `BackendEventBus.publish` を直接叩く（Step 0 設計）ため await 不要。
- **fetch_account の例外**: reducer_bridge は「例外でループ終了 + last_error 記録」だが、
  口座同期で「1 回の transient 失敗で永久停止」は実運用で困る。よって本実装は
  **warning ログ + last_error 記録のうえでループ継続**（best-effort・継続性優先）し、
  正常な `CancelledError` のみで終了する。`on_account_event` 内の例外も同様に try で囲み
  ログのみ（呼び出し側責務だがループを守る）。
- `_last_emitted` は emit した snapshot のみ更新する。fetch 失敗時は前回値を保持し、
  復旧後に値が変わっていれば改めて emit される。
"""
from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from typing import Callable, Optional, Protocol

from engine.live.order_types import AccountSnapshot

_LOG = logging.getLogger(__name__)


@dataclass(frozen=True)
class LiveErrorRecord:
    """source 付きの live エラー観測点（D2 で server_grpc が読み、BackendError event へ寄せる布石）。"""
    source: str
    detail: str


class _AccountSource(Protocol):
    async def fetch_account(self) -> AccountSnapshot: ...


class AccountSync:
    """venue 口座の定期同期。起動時 1 回 + interval_s 毎に fetch し差分のみ emit。"""

    def __init__(
        self,
        adapter: _AccountSource,
        on_account_event: Callable[[AccountSnapshot], None],
        interval_s: float = 30.0,
        on_error: Optional[Callable[[LiveErrorRecord], None]] = None,
    ) -> None:
        self._adapter = adapter
        self._on_account_event = on_account_event
        self._interval_s = interval_s
        self._on_error = on_error
        self._task: Optional[asyncio.Task[None]] = None
        self._last_emitted: Optional[AccountSnapshot] = None
        self._last_error: Optional[BaseException] = None
        self._last_error_record: Optional[LiveErrorRecord] = None

    async def start(self) -> None:
        # 既に走っていれば no-op。die 済み task は再起動を許可（reducer_bridge と同 semantics）。
        if self._task is not None and not self._task.done():
            return
        self._last_error = None
        self._task = asyncio.create_task(self._run())

    async def force_resync(self) -> bool:
        """dedup を貫通して即座に 1 回 fetch + emit する（issue #29 Slice 2'）。

        Replay→Live 切替直後に Rust が PortfolioState を reset するため、backend は
        値不変でも強制的に AccountEvent を再 push する必要がある。`_tick(force_emit=True)`
        は snapshot が `_last_emitted` と同一でも emit する。fetch 失敗時は `_tick` 内の
        on_error 経路（既存）で surface され、例外を握り潰さず継続する。

        戻り値: emit に成功したら True、fetch 失敗等で emit できなければ False
        （handler が success/error_code を判定するのに使う）。"""
        return await self._tick(force_emit=True)

    async def _run(self) -> None:
        # 初期ロード: interval を待たず即 fetch + emit（必ず 1 回出す）。
        await self._tick(force_emit=True)
        while True:
            try:
                await asyncio.sleep(self._interval_s)
            except asyncio.CancelledError:
                return
            await self._tick(force_emit=False)

    async def _tick(self, *, force_emit: bool) -> bool:
        """1 回 fetch + emit を試みる。emit に成功したら True、fetch 失敗や
        callback 失敗で emit できなかった場合は False を返す（dedup skip も False）。"""
        try:
            snapshot = await self._adapter.fetch_account()
        except asyncio.CancelledError:
            raise
        except BaseException as exc:  # noqa: BLE001 — best-effort: 1 回失敗で停止させない
            self._last_error = exc
            detail = f"{type(exc).__name__}: {exc}" if str(exc) else repr(exc)
            record = LiveErrorRecord(source="account_sync", detail=detail)
            self._last_error_record = record
            _LOG.warning("AccountSync: fetch_account failed, continuing", exc_info=exc)
            if self._on_error is not None:
                try:
                    self._on_error(record)
                except asyncio.CancelledError:
                    raise
                except BaseException:  # noqa: BLE001 — on_error の失敗でループを止めない
                    _LOG.warning("AccountSync: on_error callback failed", exc_info=True)
            return False

        if not force_emit and snapshot == self._last_emitted:
            return False  # 不変なら emit しない（差分 push）

        try:
            self._on_account_event(snapshot)
        except asyncio.CancelledError:
            raise
        except BaseException as exc:  # noqa: BLE001 — callback の失敗でループを止めない
            # `_last_emitted` は **成功時のみ** 更新する。ここで先に更新してしまうと、
            # 配信に失敗した snapshot を「emit 済み」と誤記録し、値が変わるまで二度と
            # 再送されない（特に force_emit=True の初回ロードが永久に欠落しうる）。
            _LOG.warning("AccountSync: on_account_event callback failed", exc_info=exc)
            return False
        self._last_emitted = snapshot
        return True

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

    @property
    def last_error_record(self) -> Optional[LiveErrorRecord]:
        return self._last_error_record
