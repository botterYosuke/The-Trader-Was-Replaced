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

class OhlcPoint(_BoundaryModel):
    timestamp_ms: int = Field(..., gt=0, description="バー終値時刻 (ms)")
    open_time_ms: int = Field(..., ge=0, description="バー開始時刻 (ms)")
    open: float = Field(..., gt=0, description="始値")
    high: float = Field(..., gt=0, description="高値")
    low: float = Field(..., gt=0, description="安値")
    close: float = Field(..., gt=0, description="終値")
    volume: Optional[float] = Field(None, ge=0, description="バー出来高。None=未提供 (zero-volume と区別)")

    @field_validator("open", "high", "low", "close")
    @classmethod
    def check_finite(cls, v: float) -> float:
        if not math.isfinite(v): raise ValueError("Price must be finite")
        return v

class DepthLevel(_BoundaryModel):
    price: float = Field(..., gt=0, description="板の価格")
    size: float = Field(..., ge=0, description="その価格の数量")

class DepthSnapshot(_BoundaryModel):
    bids: List[DepthLevel] = Field(default_factory=list, description="買い板 (price 降順想定)")
    asks: List[DepthLevel] = Field(default_factory=list, description="売り板 (price 昇順想定)")
    timestamp_ms: Optional[int] = Field(None, description="板 snapshot 時刻 (ms)")

class PerInstrumentState(_BoundaryModel):
    price: Optional[float] = Field(None, gt=0, description="その銘柄の最新価格")
    ohlc_points: List[OhlcPoint] = Field(default_factory=list, description="その銘柄の OHLC バー履歴")
    depth: Optional[DepthSnapshot] = Field(None, description="その銘柄の最新板 (Live のみ; Replay では None)")

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
    ohlc_points: List[OhlcPoint] = Field(default_factory=list, description="OHLC バー履歴")
    open: Optional[float] = Field(None, description="バー始値")
    high: Optional[float] = Field(None, description="バー高値")
    low: Optional[float] = Field(None, description="バー安値")
    close: Optional[float] = Field(None, description="バー終値 (price と同値)")
    open_time_ms: Optional[int] = Field(None, description="バー開始時刻 (ms)")
    replay_state: Optional[str] = Field(None, description="リプレイ状態 (IDLE/LOADED/RUNNING/PAUSED)")
    execution_mode: Literal["Replay", "LiveManual", "LiveAuto"] = Field(
        "Replay", description="実行モード (Replay=過去再生, LiveManual=実発注手動, LiveAuto=実発注自動)"
    )
    venue_state: Optional[str] = Field(None, description="Venue 接続状態 (例: CONNECTED/DISCONNECTED)")
    venue_id: Optional[str] = Field(None, description="接続中の Venue 識別子 (例: TACHIBANA)")
    configured_venue: Optional[str] = Field(
        None,
        description="バックエンド起動時の --live-venue 設定 venue (例: TACHIBANA / KABU)。未設定なら None"
    )
    subscribed_instruments: List[str] = Field(default_factory=list, description="購読中の銘柄シンボル一覧")
    instruments_loaded: int = Field(0, ge=0, description="ListInstruments で読み込んだ銘柄件数 (Rust BackendStatusUpdate::VenueChanged.instruments_loaded 配線元)")

    last_prices: dict[str, float] = Field(default_factory=dict, description="Live モードの最新価格 snapshot (quote_mid 優先 / last_trade fallback)。Replay モードでは常に空 dict。")
    per_instrument: dict[str, PerInstrumentState] = Field(default_factory=dict, description="銘柄シンボル → その銘柄ごとの状態 (price/ohlc/depth)")
    # §9.14 ADR: live_last_error は必ず TradingState の最後の field に置く
    # (UI / Rust 側 deserializer が末尾追加を許容する optional field として扱うため)。
    live_last_error: Optional[str] = Field(None, description="Live runner/bridge の最終エラー (type名: message)")

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
