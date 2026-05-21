"""engine.live.strategy_registry — strategy_id ↔ 検証済みファイルの台帳 (Phase 10 §2.5)。

`RegisterLiveStrategy`（gRPC, Step 3）は保存済み `.py` を受け取り、ロードして
検証し、`StartLiveStrategy` に渡す opaque な `strategy_id` を発行する。
`StartLiveStrategy` は生パスを受け取らず `strategy_id` だけを受け取る（任意ホスト
パスを live auto 経路で exec させない、M9）。

**path 検証ポリシー = Replay と同じ**（ユーザー決定, 2026-05-21）:
Replay の `StartEngine` は `strategy_file` を特別な許可 root に閉じ込めず、
`strategy_loader.load()` でロードできるかだけを検証している。Live 起動も同じ扱いに
揃える——`resolve()` した実ファイルを `strategy_loader.load()` でロードできれば検証
成功とする。許可 root の allow-list は導入しない（計画書 §2.5 の「許可ディレクトリ
配下」要件はこの決定で緩和、Replay と非対称な制約を作らない）。

`strategy_id` は内容ハッシュ由来（`strat-{sha256[:16]}`）で、同じファイルの再登録は
同じ id に解決される（冪等）。台帳は in-memory・永続化なし（プロセス再起動で消える）。
"""

from __future__ import annotations

import hmac
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from engine.strategy_runtime import strategy_loader


class StrategyRegistryError(Exception):
    """登録/解決の既知エラー。`error_code` を gRPC Res にそのまま載せる。

    error_code: STRATEGY_FILE_NOT_FOUND / STRATEGY_NOT_A_FILE /
    STRATEGY_LOAD_FAILED / STRATEGY_HASH_MISMATCH / UNKNOWN_STRATEGY_ID。
    """

    def __init__(self, error_code: str) -> None:
        super().__init__(error_code)
        self.error_code = error_code


@dataclass(frozen=True)
class StrategyHandle:
    strategy_id: str
    resolved_path: str
    sha256: str
    display_name: str
    scenario: dict


class StrategyRegistry:
    """`RegisterLiveStrategy` 用の strategy_id ↔ 検証済みハンドル台帳。"""

    def __init__(self, loader: Callable[[Any], tuple] = strategy_loader.load) -> None:
        self._loader = loader
        self._handles: dict[str, StrategyHandle] = {}
        self._lock = threading.Lock()

    def register(self, strategy_file: str, expected_sha256: str = "") -> StrategyHandle:
        """`.py` を検証してハンドルを発行する（Replay と同じ load 検証）。

        Raises:
            StrategyRegistryError: 不在 / 非ファイル / load 失敗 / hash 不一致。
        """
        if not strategy_file:
            raise StrategyRegistryError("STRATEGY_FILE_NOT_FOUND")

        try:
            resolved = Path(strategy_file).resolve(strict=True)
        except (FileNotFoundError, OSError) as exc:
            raise StrategyRegistryError("STRATEGY_FILE_NOT_FOUND") from exc
        if not resolved.is_file():
            raise StrategyRegistryError("STRATEGY_NOT_A_FILE")

        sha256 = strategy_loader.sha256_file(resolved)
        # TOCTOU ガード: 確認モーダルが見た内容と起動時の実ファイルが一致するか。
        if expected_sha256 and not hmac.compare_digest(expected_sha256, sha256):
            raise StrategyRegistryError("STRATEGY_HASH_MISMATCH")

        # Replay と同じく load できることを検証（許可 root の allow-list は持たない）。
        try:
            _module, scenario, strategy_cls = self._loader(str(resolved))
        except FileNotFoundError as exc:
            raise StrategyRegistryError("STRATEGY_FILE_NOT_FOUND") from exc
        except Exception as exc:  # noqa: BLE001 — load 失敗は構造化エラーに正規化
            raise StrategyRegistryError("STRATEGY_LOAD_FAILED") from exc

        handle = StrategyHandle(
            strategy_id=f"strat-{sha256[:16]}",
            resolved_path=str(resolved),
            sha256=sha256,
            display_name=getattr(strategy_cls, "__name__", resolved.stem),
            scenario=scenario,
        )
        with self._lock:
            self._handles[handle.strategy_id] = handle
        return handle

    def resolve(self, strategy_id: str) -> StrategyHandle:
        """発行済み strategy_id をハンドルに解決する。

        Raises:
            StrategyRegistryError: UNKNOWN_STRATEGY_ID（未登録 / プロセス再起動後）。
        """
        with self._lock:
            handle = self._handles.get(strategy_id)
        if handle is None:
            raise StrategyRegistryError("UNKNOWN_STRATEGY_ID")
        return handle
