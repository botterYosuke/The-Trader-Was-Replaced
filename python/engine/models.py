from pydantic import BaseModel, Field, ConfigDict, field_validator
from typing import List, Optional, Literal
import time
import math

class _BoundaryModel(BaseModel):
    model_config = ConfigDict(
        strict=True, 
        extra="forbid", 
        frozen=True,
        allow_inf_nan=False
    )

class HistoryPoint(_BoundaryModel):
    timestamp_ms: int = Field(..., gt=0, description="Unix タイムスタンプ (ミリ秒)")
    price: float = Field(..., gt=0, description="その時刻の価格")

    @field_validator("price")
    @classmethod
    def check_finite(cls, v: float) -> float:
        if not math.isfinite(v): raise ValueError("Price must be finite")
        return v

class TradingState(_BoundaryModel):
    price: float = Field(..., description="現在の市場価格", gt=0)
    history: List[float] = Field(default_factory=list, description="過去の価格履歴")
    timestamp: float = Field(
        default_factory=time.time,
        description="データ生成時の Unix タイムスタンプ (秒)",
        gt=0
    )
    timestamp_ms: Optional[int] = Field(None, description="Source of Truth (ms)")
    history_points: List[HistoryPoint] = Field(default_factory=list, description="詳細な履歴ポイント")
    open: Optional[float] = Field(None, description="バー始値")
    high: Optional[float] = Field(None, description="バー高値")
    low: Optional[float] = Field(None, description="バー安値")
    close: Optional[float] = Field(None, description="バー終値 (price と同値)")
    open_time_ms: Optional[int] = Field(None, description="バー開始時刻 (ms)")

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
    mode: Literal["static", "replay"] = Field("static", description="実行モード")
