# Phase 5: Chart Data and Enhanced Backend - Implementation Plan (Finalized)

## 概要

Phase 5 では、チャート表示の品質向上と安定性を目指し、バックエンドからのデータ供給を強化するとともに、Bevy 側でのチャート描画基盤を構築します。  
`e-station` の設計思想を継承しつつ、Epoch 0 (1970年) 描画バグなどの既知の課題を解決し、Phase 6 のリプレイ/Nautilus 連携に向けた「時刻の Source of Truth」を確立します。

## 1. データ構造と時刻の正規化

### 1.1 時刻正規化ルール
- **入力 (CSV/Replay)**: `float` (seconds) を許容し、`DataEngine` 内部で `int` (ms) に即時変換。
- **内部状態 / Wire (gRPC)**: 常に `int` (Unix milliseconds) を主軸とする。
- **後方互換**: 秒単位の `timestamp: float` も維持。

### 1.2 Python データモデル (`python/engine/models.py`)

```python
class HistoryPoint(_BoundaryModel):
    timestamp_ms: int = Field(..., gt=0, description="Unix タイムスタンプ (ミリ秒)")
    price: float = Field(..., gt=0, description="その時刻の価格")

    @field_validator("price")
    @classmethod
    def check_finite(cls, v):
        if not math.isfinite(v): raise ValueError("Price must be finite")
        return v

class TradingState(_BoundaryModel):
    price: float
    history: List[float]
    timestamp: float # Unix seconds
    
    # 新規主契約: 欠損時は None で受け、ロジック側で正規化
    timestamp_ms: Optional[int] = Field(None, description="Source of Truth (ms)")
    history_points: List[HistoryPoint] = Field(default_factory=list)
```

### 1.3 Rust データモデル (`src/trading.rs`)

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendTradingState {
    pub price: f32,
    pub history: Vec<f32>,
    pub timestamp: f64,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub history_points: Vec<HistoryPoint>,
}
```

## 2. Python バックエンドの強化

### 2.1 パラメータの伝搬ルート
1. **CLI**: `--max-history-len`, `--advance-interval-sec` を `__main__.py` でパース。
2. **Server**: `serve(..., max_history_len, advance_interval_sec)` へ渡す。
3. **Engine**: `DataEngine(max_history_len=...)` を初期化。
4. **Loop**: `advance_loop(engine, advance_interval_sec)` で進行。

### 2.2 正規化と同期更新 (`core.py`)
- `get_current_state()` 内で `timestamp_ms` が未設定の場合は `int(timestamp * 1000)` で埋めて返す。
- `advance()` 時、`price` と `timestamp_ms` を同時に append/pop し、長さを常に一致させる。

## 3. Bevy (Rust) チャートシステムの構築

### 3.1 描画基盤
- **描画技術**: `bevy_vector_shapes` (Painter) を使用。
- **Component 単位の状態管理**: `ChartViewState` を各チャート Entity に持たせる。

### 3.2 Epoch 0 回避と初期化ルール
- **初期化**: `ChartViewState.latest_timestamp_ms` の初期値は 0 とせず、最初の有効なデータ（`history_points.last()`）を受信した時点で設定。
- **描画スキップ**: `latest_timestamp_ms == 0` または `history_points` が空の場合は描画をスキップし、Epoch 0 への描画を防止。
- **シミュレーション同期**: `price_simulation_system` も `history_points` を更新するように修正し、バックエンド無効時もチャートを機能させる。

## 4. 実装ステップ

1. **[Python] Data Normalization**: `HistoryPoint` 導入と `DataEngine` での ms 変換・同期更新。
2. **[Python] Param Plumbing**: CLI から `advance_loop` までの引数伝搬の実装。
3. **[Rust] Backend Sync**: `TradingData` への新フィールド追加と `timestamp` 秒からのフォールバック実装。
4. **[Rust] Chart Component**: `src/ui/chart.rs` 新設。Component ベースの `ChartViewState` 管理。
5. **[Rust] Painter System**: `bevy_vector_shapes` による描画、Autoscale、シミュレーション対応。

## 5. 検証

- **後方互換テスト**: `timestamp_ms` 欠損の旧 JSON からも ms 軸が正しく描画されること。
- **正規化テスト**: CSV 入力 (seconds) が wire 上で正しい ms (int) に変換されていること。
- **Epoch 0 テスト**: データ受信前や不正データ時に 1970 年が表示されないこと。
- **同期テスト**: `history_points` の長さが `max-history-len` 内で正しく管理されていること。
