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
    """

    order_id: str
    venue_order_id: str
    client_order_id: str
    status: str
    filled_qty: float
    avg_price: float
    ts_ms: int

    model_config = {"frozen": True}
