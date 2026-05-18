"""Tests for kabusapi_ws_codec.KabuPushFrameProcessor (Phase 8 §3.2 B4-3).

kabu PUSH JSON 1 メッセージ (= 1 銘柄スナップショット) を
``(trade_dict | None, depth_dict | None)`` に正規化する stateful processor の
contract test 群。tachibana の ``FdFrameProcessor`` と境界を揃える。

設計確認 (ユーザー確定):
- codec は dict を返す (pydantic 化は B4-4 で KabuStationAdapter が実施)。
- "unknown" aggressor_side は本 repo では禁止 (TradesUpdate.aggressor_side
  は Literal["buy", "sell"])。codec は根拠なしの trade を None で抑制する。
- depth は best-effort で常に発行 (空 bids/asks も dict として返す)。
- trade は TradingVolume の正の差分があり、かつ side を確定できたときのみ発行。
- DV reset / first frame は trade=None で state 再初期化。

注:
- ``asyncio_mode = "auto"`` 設定済だが、本 SUT は pure (non-async) なので mark 不要。
- SUT (``engine.exchanges.kabusapi_ws_codec``) は B4-3 RED 時点で未実装。
  top-level import すると collection error になるため、各 test 内で
  deferred import する (tachibana の test と同じ慣例)。
- dict のキー名は B4-4 で adapter が ``TradesUpdate`` / ``DepthUpdate``
  (``ts_ns``, ``price``, ``size``, ``aggressor_side``, ``bids: tuple[DepthLevel,...]``)
  に流せる形:

    depth_dict = {
        "symbol": str,
        "ts_ns": int,                 # UTC ns epoch
        "bids": [(price: float, size: float), ...],  # Buy1..Buy10 順
        "asks": [(price: float, size: float), ...],  # Sell1..Sell10 順
    }
    trade_dict = {
        "symbol": str,
        "ts_ns": int,
        "price": float,
        "size": float,
        "aggressor_side": "buy" | "sell",
    }
"""
from __future__ import annotations

from typing import Any


# ---------------------------------------------------------------------------
# Sample frame builder (inline per-test; no shared fixture by design)
# ---------------------------------------------------------------------------


def _frame(
    *,
    symbol: str = "7203",
    current_price: float | None = 1000.0,
    current_price_time: str | None = "2024-07-01T09:00:00+09:00",
    trading_volume: float | None = 100.0,
    asks: list[tuple[float, float] | None] | None = None,
    bids: list[tuple[float, float] | None] | None = None,
) -> dict[str, Any]:
    """Build a kabu PUSH JSON-ish dict.

    ``asks`` / ``bids`` は ``[(price, qty), ...]`` の順 (1..N)。
    各 entry を ``None`` にするとその段を欠損 (key 自体は None) として配置する。
    指定がない場合は best=1000 を中心とした 3 段ダミーを置く。
    """
    if asks is None:
        asks = [(1001.0, 10.0), (1002.0, 20.0), (1003.0, 30.0)]
    if bids is None:
        bids = [(999.0, 10.0), (998.0, 20.0), (997.0, 30.0)]

    frame: dict[str, Any] = {
        "Symbol": symbol,
        "CurrentPrice": current_price,
        "CurrentPriceTime": current_price_time,
        "TradingVolume": trading_volume,
    }
    for i, level in enumerate(asks, start=1):
        if level is None:
            frame[f"Sell{i}"] = None
        else:
            p, q = level
            frame[f"Sell{i}"] = {"Price": p, "Qty": q, "Time": None, "Sign": None}
    for i, level in enumerate(bids, start=1):
        if level is None:
            frame[f"Buy{i}"] = None
        else:
            p, q = level
            frame[f"Buy{i}"] = {"Price": p, "Qty": q, "Time": None, "Sign": None}
    return frame


# ---------------------------------------------------------------------------
# (1) first frame: trade=None, depth は発行
# ---------------------------------------------------------------------------


def test_first_frame_emits_depth_but_no_trade():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    trade, depth = proc.process(_frame(trading_volume=100.0))

    assert trade is None
    assert depth is not None
    assert depth["symbol"] == "7203"
    assert depth["asks"][0] == (1001.0, 10.0)
    assert depth["bids"][0] == (999.0, 10.0)


# ---------------------------------------------------------------------------
# (2) 2nd frame で TradingVolume が +N 増えていれば trade 発行 size==N
# ---------------------------------------------------------------------------


def test_volume_increase_emits_trade_with_diff_size():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    # 2nd frame: +15, price=1001 (>= prev_ask1=1001) → buy
    trade, depth = proc.process(
        _frame(trading_volume=115.0, current_price=1001.0)
    )

    assert trade is not None
    assert trade["symbol"] == "7203"
    assert trade["price"] == 1001.0
    assert trade["size"] == 15.0
    assert trade["aggressor_side"] == "buy"
    assert depth is not None


# ---------------------------------------------------------------------------
# (3) 2nd frame で TradingVolume 変化なし → trade=None / depth は発行
# ---------------------------------------------------------------------------


def test_volume_unchanged_emits_no_trade():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    proc.process(_frame(trading_volume=100.0))
    trade, depth = proc.process(_frame(trading_volume=100.0, current_price=1000.5))

    assert trade is None
    assert depth is not None


# ---------------------------------------------------------------------------
# (4) CurrentPrice >= prev_ask1 → aggressor_side="buy"
# ---------------------------------------------------------------------------


def test_aggressor_buy_when_price_at_or_above_prev_ask1():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    # 1st: prev_ask1=1001, prev_bid1=999
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    # 2nd: price=1001 == prev_ask1 → buy
    trade, _ = proc.process(_frame(trading_volume=110.0, current_price=1001.0))
    assert trade is not None
    assert trade["aggressor_side"] == "buy"


# ---------------------------------------------------------------------------
# (5) CurrentPrice <= prev_bid1 → aggressor_side="sell"
# ---------------------------------------------------------------------------


def test_aggressor_sell_when_price_at_or_below_prev_bid1():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    # 2nd: price=999 == prev_bid1 → sell
    trade, _ = proc.process(_frame(trading_volume=110.0, current_price=999.0))
    assert trade is not None
    assert trade["aggressor_side"] == "sell"


# ---------------------------------------------------------------------------
# (6) midpoint かつ直前 side が "buy" → 維持して "buy"
# ---------------------------------------------------------------------------


def test_aggressor_midpoint_inherits_prev_side_buy():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    # frame1: state init, prev_ask=1001, prev_bid=999
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    # frame2: price=1001 → buy, _prev_side="buy"
    t2, _ = proc.process(_frame(trading_volume=110.0, current_price=1001.0))
    assert t2 is not None and t2["aggressor_side"] == "buy"
    # frame3: price=1000.5 は prev_ask(1001) 未満 / prev_bid(999) 超過 → 中間
    #         → 直前 side="buy" を維持
    t3, _ = proc.process(_frame(trading_volume=120.0, current_price=1000.5))
    assert t3 is not None
    assert t3["aggressor_side"] == "buy"
    assert t3["size"] == 10.0


# ---------------------------------------------------------------------------
# (7) midpoint かつ直前 side も None → trade=None
# ---------------------------------------------------------------------------


def test_aggressor_midpoint_without_prev_side_drops_trade():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    # frame1: init, prev_ask=1001, prev_bid=999, _prev_side=None
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    # frame2: price=1000 (中間) かつ _prev_side=None → trade 抑制
    trade, depth = proc.process(_frame(trading_volume=110.0, current_price=1000.0))

    assert trade is None
    assert depth is not None  # depth は常に発行


# ---------------------------------------------------------------------------
# (8) TradingVolume reset → trade=None、state 再初期化
# ---------------------------------------------------------------------------


def test_volume_reset_drops_trade_and_reinitializes_state():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    proc.process(_frame(trading_volume=200.0, current_price=1001.0))  # buy 確定
    # reset frame: TradingVolume が前回より小さい → 休場明け等
    trade, depth = proc.process(_frame(trading_volume=50.0, current_price=1001.0))
    assert trade is None
    assert depth is not None
    # 次 frame は first-frame 相当: prev_volume=50 からの差分で size を計算する
    trade2, _ = proc.process(_frame(trading_volume=60.0, current_price=1001.0))
    assert trade2 is not None
    assert trade2["size"] == 10.0


# ---------------------------------------------------------------------------
# (9) reset() で全 state が初期化 (次 frame は first-frame 扱い → trade=None)
# ---------------------------------------------------------------------------


def test_explicit_reset_returns_to_first_frame_behavior():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    proc.process(_frame(trading_volume=100.0, current_price=1000.0))
    proc.process(_frame(trading_volume=200.0, current_price=1001.0))

    proc.reset()
    # reset 直後の最初の frame は trade=None (volume diff も side 推定も使わない)
    trade, depth = proc.process(_frame(trading_volume=300.0, current_price=1001.0))
    assert trade is None
    assert depth is not None


# ---------------------------------------------------------------------------
# (10) Sell/Buy 欠損段は skip され depth の段数が縮む
# ---------------------------------------------------------------------------


def test_depth_skips_missing_levels():
    from engine.exchanges.kabusapi_ws_codec import KabuPushFrameProcessor

    proc = KabuPushFrameProcessor(symbol="7203")
    # Sell2 が None entry、Sell3 は Qty=None で skip。Sell1 のみ残る (asks=1段)
    asks = [
        (1001.0, 10.0),
        None,
        (1003.0, None),  # Qty=None → skip
    ]
    bids = [(999.0, 10.0), (998.0, 20.0)]
    trade, depth = proc.process(
        _frame(trading_volume=100.0, asks=asks, bids=bids)
    )

    assert trade is None  # first frame
    assert depth is not None
    assert len(depth["asks"]) == 1
    assert depth["asks"][0] == (1001.0, 10.0)
    assert len(depth["bids"]) == 2
