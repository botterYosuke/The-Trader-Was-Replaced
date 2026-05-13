# Phase 6: Nautilus Replay Integration - Implementation Plan

## 概要

Phase 6 では、独自の簡易 CSV リプレイから **Nautilus Trader** エンジンベースのリプレイへと移行します。  
これにより、板情報（OrderBook）や約定イベントを含む、より高度なバックテスト環境への道筋を付けます。  
Phase 5 で確立した `timestamp_ms` 主軸のデータモデルを継承しつつ、gRPC API を拡張してリプレイ制御（一時停止、速度変更、ステップ実行）を実現します。

## 1. 概念設計

### 1.1 Nautilus as Source of Truth
- **Engine**: `NautilusRunner` (Python) が Nautilus の `BacktestEngine` を管理。
- **Event Driven**: Nautilus からのデータイベント（Ticks, Bars）を受け取り、`TradingState` を更新（Reduction）。
- **Polling Sync**: Rust (Bevy) は引き続き `GetState` を一定間隔で呼び出し、最新の `TradingState` を取得する。

### 1.2 gRPC API の拡張 (Additive)
既存の `Start` / `Stop` / `GetState` は維持しつつ、以下のコマンドを追加します。
- `LoadReplayData`: リプレイ対象の銘柄、期間、データソースを指定。
- `PauseReplay` / `ResumeReplay`: リプレイの一時停止と再開。
- `SetReplaySpeed`: リプレイ速度（倍率）の変更。
- `StepReplay`: 1ステップ（または1データポイント）分だけ進める。

## 2. Python バックエンドの強化

### 2.1 NautilusRunner の導入
`e-station` の設計を参考に、最小限の `NautilusRunner` を `python/engine/nautilus/` に実装します。
- `BacktestEngine` の初期化。
- `ParquetData` または `CSVData` のロード。
- エンジンイベントのコールバック処理。

### 2.2 DataEngine のリファクタリング
- `DataEngine` が `NautilusRunner` を保持し、モードを `static` から `nautilus_replay` へ切り替え可能にする。
- `advance()` メソッドを Nautilus のイベントループまたはステップ実行と同期させる。

### 2.3 State Reducer
- Nautilus の `DataType` (TradeTick, Bar) を `HistoryPoint` に変換。
- `TradingState.history_points` への蓄積と `max_history_len` による制限。

## 3. gRPC 通信仕様の更新

### 3.1 `engine.proto`
```proto
service DataEngine {
  // 既存
  rpc GetState(GetStateRequest) returns (GetStateResponse);
  rpc Start(StartRequest) returns (StartResponse);
  rpc Stop(StopRequest) returns (StopResponse);

  // 新規追加 (Phase 6)
  rpc LoadReplayData(LoadReplayDataRequest) returns (LoadReplayDataResponse);
  rpc SetReplaySpeed(SetReplaySpeedRequest) returns (SetReplaySpeedResponse);
  rpc PauseReplay(PauseReplayRequest) returns (PauseReplayResponse);
  rpc ResumeReplay(ResumeReplayRequest) returns (ResumeReplayResponse);
  rpc StepReplay(StepReplayRequest) returns (StepReplayResponse);
}

message LoadReplayDataRequest {
  string token = 1;
  string instrument_id = 2;
  string start_date = 3; // ISO8601
  string end_date = 4;   // ISO8601
  string data_source = 5;
}
// ... その他のメッセージ定義
```

## 4. 実装ステップ

### 4.1 Step 1: Nautilus 基盤の実装
- `python/engine/nautilus/` フォルダの作成。
- `NautilusRunner` のプロトタイプ実装（Headless 実行）。
- Nautilus の Tick データを `TradingState` に変換するロジックの実装。

### 4.2 Step 2: gRPC API の拡張
- `engine.proto` の更新とコード生成。
- `server_grpc.py` への新規メソッドの実装。
- `DataEngine` への制御フラグ（`is_paused`, `replay_speed`）の追加。

### 4.3 Step 3: Bevy 側の UI 連携 (Optional)
- 拡張された gRPC メソッドを呼び出すための Rust クライアントコードの更新。
- （必要に応じて）UI 上に一時停止や速度変更のボタンを追加。

## 5. 検証

- **互換性テスト**: 既存の `GetState` / `Start` / `Stop` が Nautilus モードでも正しく動作すること。
- **制御テスト**: `Pause` 呼び出し後に `GetState` で取得される `timestamp_ms` が停止すること。
- **速度テスト**: `SetReplaySpeed` で `advance_loop` の間隔または Nautilus の進行が変化すること。
- **精度テスト**: Nautilus が供給する価格データが `history_points` に正しく反映されていること。

## 6. 注意点

- **Streaming の保留**: Bidirectional Streaming への移行は Phase 6 では行わず、引き続き Polling を主軸とする。
- **Nautilus 依存**: `nautilus_trader` パッケージが必要になるため、`pyproject.toml` / `uv.lock` の更新を伴う。
- **時刻の Source of Truth**: 常に Nautilus の `Clock` から取得される時刻を `timestamp_ms` として扱う。
