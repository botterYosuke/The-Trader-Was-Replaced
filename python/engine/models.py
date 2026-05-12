from pydantic import BaseModel, Field, ConfigDict, field_validator
from typing import List
import time
import math

class _BoundaryModel(BaseModel):
    model_config = ConfigDict(
        strict=True, 
        extra="forbid", 
        frozen=True,
        allow_inf_nan=False
    )

class TradingState(_BoundaryModel):
    price: float = Field(..., description="現在の市場価格", gt=0)
    history: List[float] = Field(default_factory=list, description="過去の価格履歴")
    timestamp: float = Field(
        default_factory=time.time, 
        description="データ生成時の Unix タイムスタンプ (秒)",
        gt=0
    )

    @field_validator("history")
    @classmethod
    def check_history_finite(cls, v: List[float]) -> List[float]:
        if any(not math.isfinite(x) for x in v):
            raise ValueError("History contains non-finite values")
        return v
