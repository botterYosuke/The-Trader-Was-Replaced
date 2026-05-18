"""kabu STATION PUSH JSON frame → (trade, depth) normalizer (Phase 8 §3.2 B4-3).

tachibana ``FdFrameProcessor`` (event WS) の kabu PUSH 版。
1 銘柄スナップショット (1 JSON message) を ``(trade_dict | None, depth_dict | None)``
に正規化する stateful processor。

Contract (B4-3 確定仕様):

- depth は best-effort で **常に発行** (bids/asks 空 list も dict として返す)。
- trade は次を **全て** 満たすときだけ発行:
    1. ``_prev_volume is not None`` (= first/reset frame ではない)
    2. ``current_volume > _prev_volume`` (= volume diff が正)
    3. aggressor_side が ``"buy"`` / ``"sell"`` に確定する
       (中間値かつ ``_prev_side is None`` のときは抑制)
- 本 repo 規約: ``"unknown"`` aggressor_side は禁止。
- side 判定 (quote rule, 直前 frame の prev_ask1/prev_bid1 に対して):
    * ``current_price >= _prev_ask1`` → ``"buy"``
    * ``current_price <= _prev_bid1`` → ``"sell"``
    * それ以外 (中間) かつ ``_prev_side is not None`` → ``_prev_side`` 維持
    * それ以外かつ ``_prev_side is None`` → trade 抑制 (None)
- state 更新は trade 生成の **後** に行う (= tachibana と同じ quote rule)。
- ``ts_ns``: kabu PUSH ``CurrentPriceTime`` (JST ISO8601) を UTC ns epoch int に。
  tzinfo 無しは JST 仮定。``None`` / 空文字は ``ts_ns=None``。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any, Literal

JST = timezone(timedelta(hours=9))


# ---------------------------------------------------------------------------
# Helpers (module-private)
# ---------------------------------------------------------------------------


def _parse_jst_to_utc_ns(raw_ts: str | None) -> int | None:
    """Convert kabu ``CurrentPriceTime`` to UTC ns epoch int.

    - ``None`` / 空文字 → ``None``
    - tzinfo 無しの ISO8601 → JST 仮定
    - parse 失敗 → ``None`` (caller が ts_ns=None で発行)
    """
    if not raw_ts:
        return None
    try:
        dt = datetime.fromisoformat(raw_ts)
    except ValueError:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=JST)
    dt_utc = dt.astimezone(timezone.utc)
    return int(dt_utc.timestamp() * 1_000_000_000)


def _collect_levels(
    frame: dict[str, Any], prefix: str
) -> list[tuple[float, float]]:
    """Collect ``[(price, qty), ...]`` from ``{prefix}1``..``{prefix}10``.

    欠損段 (entry が ``None``, Price/Qty が ``None``) は skip。
    """
    levels: list[tuple[float, float]] = []
    for i in range(1, 11):
        entry = frame.get(f"{prefix}{i}")
        if entry is None:
            continue
        price = entry.get("Price")
        qty = entry.get("Qty")
        if price is None or qty is None:
            continue
        levels.append((float(price), float(qty)))
    return levels


def _best_or_none(levels: list[tuple[float, float]]) -> float | None:
    """Return ``levels[0][0]`` (best price) or ``None`` if empty."""
    return levels[0][0] if levels else None


# ---------------------------------------------------------------------------
# Frame processor
# ---------------------------------------------------------------------------


@dataclass
class KabuPushFrameProcessor:
    """Stateful per-symbol kabu PUSH frame → (trade, depth) normalizer.

    Caller must hold one instance per ``symbol`` and call :meth:`reset` on
    WS reconnect / subscription change to avoid carrying stale DV/quote
    state across session boundaries.
    """

    symbol: str

    _prev_volume: float | None = field(default=None, init=False, repr=False)
    _prev_ask1: float | None = field(default=None, init=False, repr=False)
    _prev_bid1: float | None = field(default=None, init=False, repr=False)
    _prev_side: Literal["buy", "sell"] | None = field(
        default=None, init=False, repr=False
    )

    def reset(self) -> None:
        """Reset DV/quote/side state. Next frame is first-frame."""
        self._prev_volume = None
        self._prev_ask1 = None
        self._prev_bid1 = None
        self._prev_side = None

    def process(
        self, frame: dict[str, Any]
    ) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
        """Process one kabu PUSH JSON message.

        Returns ``(trade | None, depth | None)``. depth は best-effort で
        常に dict (bids/asks が空でも) を返す。trade は volume diff > 0
        かつ side 確定時のみ。
        """
        ts_ns = _parse_jst_to_utc_ns(frame.get("CurrentPriceTime"))
        asks = _collect_levels(frame, "Sell")
        bids = _collect_levels(frame, "Buy")

        depth: dict[str, Any] = {
            "symbol": self.symbol,
            "ts_ns": ts_ns,
            "bids": bids,
            "asks": asks,
        }

        current_price = frame.get("CurrentPrice")
        current_volume = frame.get("TradingVolume")
        cur_ask1 = _best_or_none(asks)
        cur_bid1 = _best_or_none(bids)

        trade: dict[str, Any] | None = None

        # --- first frame / DV reset: 状態初期化のみ、trade=None -------------
        if self._prev_volume is None or (
            current_volume is not None and current_volume < self._prev_volume
        ):
            self._prev_volume = (
                float(current_volume) if current_volume is not None else None
            )
            self._prev_ask1 = cur_ask1
            self._prev_bid1 = cur_bid1
            # DV reset でも _prev_side は None に戻す (first-frame と同等)
            self._prev_side = None
            return None, depth

        # --- 通常 frame ---------------------------------------------------
        if current_volume is None or current_price is None:
            # 必須 field 欠損 → trade なし。state は触らない。
            return None, depth

        volume_diff = float(current_volume) - self._prev_volume
        if volume_diff > 0:
            side = self._determine_side(float(current_price))
            if side is not None:
                trade = {
                    "symbol": self.symbol,
                    "ts_ns": ts_ns,
                    "price": float(current_price),
                    "size": volume_diff,
                    "aggressor_side": side,
                }
                self._prev_side = side

        # state 更新は trade 生成の **後** (tachibana quote rule と同じ)
        self._prev_volume = float(current_volume)
        self._prev_ask1 = cur_ask1
        self._prev_bid1 = cur_bid1

        return trade, depth

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    def _determine_side(
        self, price: float
    ) -> Literal["buy", "sell"] | None:
        """Quote rule against the *previous* frame's best ask/bid.

        中間値で ``_prev_side is None`` のときは ``None`` (= trade 抑制)。
        """
        if self._prev_ask1 is not None and price >= self._prev_ask1:
            return "buy"
        if self._prev_bid1 is not None and price <= self._prev_bid1:
            return "sell"
        # midpoint: 直前 side を維持 (None ならそのまま None で抑制)
        return self._prev_side
