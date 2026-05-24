"""Order submission result types.

OrderResult は venue adapter の submit_order が返す結果。
field は proto OrderEvent (engine.proto) と対応:
status / filled_qty / avg_price / client_order_id。
reject_reason のみ OrderResult 固有（REJECTED 時の理由文字列）。
"""

from __future__ import annotations

from pydantic import BaseModel, field_validator

# Canonical Nautilus OrderStatus member names (core/rust/model.pxd:352 / model/enums.py).
# venue adapter が返す status を契約として固定する。typo（"CANCELLED" 等）や
# 自由文字列が UI まで素通りするのを境界で止める（Step 5/6 の実 adapter ドリフト対策）。
VALID_ORDER_STATUSES: frozenset[str] = frozenset(
    {
        "INITIALIZED",
        "DENIED",
        "EMULATED",
        "RELEASED",
        "SUBMITTED",
        "ACCEPTED",
        "REJECTED",
        "CANCELED",
        "EXPIRED",
        "TRIGGERED",
        "PENDING_UPDATE",
        "PENDING_CANCEL",
        "PARTIALLY_FILLED",
        "FILLED",
    }
)


class OrderResult(BaseModel):
    """submit_order の結果。proto OrderEvent 互換 + reject_reason。"""

    status: str  # "FILLED" / "REJECTED" / "PARTIALLY_FILLED" 等（Nautilus OrderStatus name）
    filled_qty: float
    avg_price: float | None
    client_order_id: str
    reject_reason: str | None = None

    model_config = {"frozen": True}

    @field_validator("status")
    @classmethod
    def _status_must_be_nautilus_name(cls, v: str) -> str:
        if v not in VALID_ORDER_STATUSES:
            raise ValueError(
                f"invalid OrderStatus name: {v!r} "
                f"(must be one of the Nautilus OrderStatus members)"
            )
        return v


class OrderEventData(BaseModel):
    """ManualOrderFacade が返す正規化済み注文イベント（proto `OrderEvent` と field 一致）。

    facade は transport 非依存（proto を import しない）ため、gRPC handler 側で
    この dataclass を `engine_pb2.OrderEvent` に詰め替える。`order_id` は UI が
    扱う安定ハンドルで、mock では `client_order_id` と同値（venue 採番が無いため
    `venue_order_id` は空文字）。

    issue #29 Slice3a: `symbol`/`side`/`qty`/`price` は発注時の静的属性。GetOrders
    による接続/再起動後の seed で UI が完全な注文行（銘柄・売買・数量・指値）を復元
    できるよう facade が place 時に載せる（`symbol` は instrument_id、MARKET は
    `price=None`）。EC stream 由来など静的属性が不明な経路では既定値（""/0.0/None）
    のまま残り、UI 側は「非空が勝つ」マージ規則で既知の値を保持する。
    """

    order_id: str
    venue_order_id: str
    client_order_id: str
    status: str
    filled_qty: float
    avg_price: float
    ts_ms: int
    symbol: str = ""
    side: str = ""
    qty: float = 0.0
    price: float | None = None

    model_config = {"frozen": True}


class AccountPositionData(BaseModel):
    """口座の 1 保有銘柄（proto `AccountPosition` と field 一致）。

    transport 非依存（account_sync / mock が用いる正規化モデル）。gRPC handler 側で
    `engine_pb2.AccountPosition` に詰め替える。
    """

    symbol: str
    qty: int
    avg_price: float
    unrealized_pnl: float

    model_config = {"frozen": True}


class AccountSnapshot(BaseModel):
    """口座スナップショット（余力 + 建玉一覧）。proto `AccountEvent` の値部分と対応。

    **ts_ms は持たない**: 等価判定（差分 emit）から時刻を排除するため。push 時に
    gRPC handler が `int(time.time()*1000)` を採番して `engine_pb2.AccountEvent.ts_ms`
    に詰める。AccountSync は同一 snapshot の連続 emit を `==`（pydantic frozen の
    field 比較）で抑止するので、時刻がここに混じると常に「変化あり」と誤判定する。

    NaN/Inf validator は付けない（mock では発生しない。OrderResult と同方針で、
    実 venue 値の境界 sanitize は Step 5/6 adapter の責務）。
    """

    cash: float
    buying_power: float
    positions: tuple[AccountPositionData, ...]

    model_config = {"frozen": True}
