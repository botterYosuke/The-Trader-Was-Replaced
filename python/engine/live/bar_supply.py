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

Step 8 で追加: live venue の約定 (`TradesUpdate`) を Nautilus `TradeTick` に変換する
``trades_update_to_trade_tick``。`LiveDataEngine` の internal aggregation はこの
`TradeTick` 列を受けて INTERNAL `Bar` を作り、戦略の `on_bar` に届ける（§2.3 / 本丸）。
変換は instrument の precision を使うだけの純関数で、aggregation 本体には触らない。

Public API:
    to_internal_bar_type(bar_type_str) -> str
    live_bar_type(instrument_id, granularity) -> str
    trades_update_to_trade_tick(trade, instrument, seq) -> TradeTick
"""

from __future__ import annotations

from typing import Any

# Nautilus 型は module scope で import する（`catalog_data_loader` 経由で既に nautilus が
# 読まれるため新たな import コスト無し）。per-tick の `trades_update_to_trade_tick` から
# 関数内 import を消し、ホットパスの import lookup を避ける（Step 8 efficiency review）。
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import TradeId

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


def _aggressor_side(side: str):
    """venue の "buy"/"sell" を Nautilus `AggressorSide` に写像する。

    不明値は `NO_AGGRESSOR`（aggregation は side を使わないので影響しない）。
    """
    s = (side or "").strip().lower()
    if s == "buy":
        return AggressorSide.BUYER
    if s == "sell":
        return AggressorSide.SELLER
    return AggressorSide.NO_AGGRESSOR


def trades_update_to_trade_tick(
    trade: Any, instrument: Any, seq: int = 0
) -> TradeTick:
    """venue の `TradesUpdate` を Nautilus `TradeTick` に変換する（Step 8）。

    price / size は instrument の precision に丸める（`make_price` / `make_qty`）。
    ``trade_id`` は同一 ns の複数約定でも衝突しないよう ``{ts_ns}-{seq}`` で採番する
    （単一 instrument / 単一 run なので ns+seq で一意。`TradeId` は 36 文字以内）。
    aggregation は trade_id / aggressor を OHLCV 計算に使わないが、`TradeTick` の
    コンストラクタが要求するため妥当値を与える。
    """
    ts = int(trade.ts_ns)
    return TradeTick(
        instrument_id=instrument.id,
        price=instrument.make_price(trade.price),
        size=instrument.make_qty(trade.size),
        aggressor_side=_aggressor_side(getattr(trade, "aggressor_side", "")),
        trade_id=TradeId(f"{ts}-{seq}"),
        ts_event=ts,
        ts_init=ts,
    )
