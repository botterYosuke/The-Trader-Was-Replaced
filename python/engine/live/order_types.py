"""Order submission result types.

OrderResult は venue adapter の submit_order が返す結果。
field は proto OrderEvent (engine.proto) と対応:
status / filled_qty / avg_price / client_order_id。
reject_reason のみ OrderResult 固有（REJECTED 時の理由文字列）。
"""

from __future__ import annotations

from pydantic import BaseModel


class OrderResult(BaseModel):
    """submit_order の結果。proto OrderEvent 互換 + reject_reason。"""

    status: str  # "FILLED" / "REJECTED" / "PARTIALLY_FILLED" 等（Nautilus OrderStatus name）
    filled_qty: float
    avg_price: float | None
    client_order_id: str
    reject_reason: str | None = None

    model_config = {"frozen": True}
