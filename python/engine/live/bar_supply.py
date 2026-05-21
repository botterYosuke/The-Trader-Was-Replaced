"""engine.live.bar_supply — Live 戦略への Bar 供給ユーティリティ (Phase 10 Step 1)。

Replay は catalog の EXTERNAL `Bar` を `BacktestEngine` 経由で `on_bar` に流す。
Live は venue tick を Nautilus `TradeTick` 化し、`LiveDataEngine` の internal
aggregation（Nautilus 標準 `data/aggregation.pyx`）で INTERNAL `Bar` を生成して
同じ `on_bar` に届ける（ADR-B / §2.3）。

このモジュールの責務は「Replay の EXTERNAL BarType を Live の INTERNAL BarType に
読み替える」変換を 1 箇所に閉じ込めることだけ。aggregation 本体は Nautilus 標準を
使うため新規実装しない。Live host (§2.2) は戦略の `bar_type` を
``to_internal_bar_type`` で読み替えてから `LiveDataEngine` に subscribe させることで、
戦略コードを 1 行も変えずに Replay↔Live を可搬にする。

Public API:
    to_internal_bar_type(bar_type_str) -> str
    live_bar_type(instrument_id, granularity) -> str
"""

from __future__ import annotations

from engine.strategy_runtime.catalog_data_loader import bar_type_for_instrument

_EXTERNAL = "-EXTERNAL"
_INTERNAL = "-INTERNAL"


def to_internal_bar_type(bar_type_str: str) -> str:
    """EXTERNAL BarType 文字列を INTERNAL に読み替える（INTERNAL は冪等）。

    戦略は同じ `BarSpecification`（step / aggregation / price_type）を購読し続け、
    変わるのは `aggregation_source` だけ。

    Raises:
        ValueError: ``-EXTERNAL`` でも ``-INTERNAL`` でも終わらない文字列。
    """
    s = bar_type_str.strip()
    if s.endswith(_INTERNAL):
        return s
    if s.endswith(_EXTERNAL):
        return s[: -len(_EXTERNAL)] + _INTERNAL
    raise ValueError(
        f"bar_type must end with -EXTERNAL or -INTERNAL, got {bar_type_str!r}"
    )


def live_bar_type(instrument_id: str, granularity: str) -> str:
    """(instrument_id, granularity) → Live 用 INTERNAL BarType 文字列。

    Replay 側 ``bar_type_for_instrument()`` の INTERNAL 版。

    >>> live_bar_type("1301.TSE", "Minute")
    '1301.TSE-1-MINUTE-LAST-INTERNAL'
    """
    return to_internal_bar_type(bar_type_for_instrument(instrument_id, granularity))
