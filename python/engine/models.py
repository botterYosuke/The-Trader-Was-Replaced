from pydantic import BaseModel, Field, ConfigDict, field_validator
from typing import List, Optional
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

class EngineSnapshot(_BoundaryModel):
    """エンジンの実行コンテキストの保存・復元用スナップショット"""
    state: TradingState = Field(..., description="現在のトレーディング状態")
    replay_index: int = Field(0, description="リプレイの現在インデックス", ge=0)
    source_path: Optional[str] = Field(None, description="データのソースパス")
    mode: str = Field("static", description="実行モード (static | replay)")
