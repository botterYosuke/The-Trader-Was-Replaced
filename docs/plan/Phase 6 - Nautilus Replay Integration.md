# Phase 6: Nautilus Replay Integration - Implementation Plan

## 概要

Phase 6 では、独自の簡易 CSV リプレイから **Nautilus Trader** エンジンベースのリプレイへと移行します。  
`e-station` のリプレイ制御ロジックをプロジェクトの Unary gRPC アーキテクチャに適合させつつ移植し、堅牢な **Replay State Machine** を導入します。  
`PAUSED` 状態を全ての層（Proto, Python Schema, Rust Enum）で正式な一級市民として扱い、確実なセッション制御を実現します。

## 1. 概念設計

### 1.1 Replay State Machine (Strict & Formalized)
`e-station` の不整合を修正し、`PAUSED` を全レイヤーで正式採用します。
- `IDLE`: 初期状態、またはリプレイセッション終了後。
- `LOADED`: `LoadReplayData` 完了（データ存在確認済）、開始準備完了。
- `RUNNING`: 戦略実行中。Nautilus がデータを供給中。
- `PAUSED`: 一時停止中。`StepReplay` はこの状態でのみ受理。
- `STOPPING`: 停止処理中。完了後に `IDLE` へ遷移。

### 1.2 Nautilus as Source of Truth (Reducer Pattern)
- **Streaming Subset**: `e-station` の `start_backtest_replay_streaming` の挙動を移植。
- **Per-item Emit**: 1件処理ごとにイベントを発火。
  - `ReplayTimeUpdated`: `timestamp_ms` を更新。
  - `Trades` / `KlineUpdate`: 価格データと履歴を更新。
- **Snapshot Reducer**:
  - `ReplayTimeUpdated.timestamp_ms`
  - `KlineUpdate.kline.open_time_ms`
  - `Trades.trades[].ts_ms`
  これらを Source of Truth として `TradingState` に集約。

## 2. Python バックエンドの強化

### 2.1 Nautilus & J-Quants Loader
- `jquants_loader.py` を移植。`base_dir` の明示指定または `JQUANTS_DIR` 環境変数を必須とし、`S:/j-quants` への黙示的フォールバックを排除。
- `LoadReplayData` の挙動: データ存在確認 (`check_data_exists`) を行い、成功すれば状態を `IDLE -> LOADED` に遷移させる。実データのロードと件数算出は後続の `StartEngine` 以降で行う（`e-station` の挙動に準拠）。

### 2.2 DataEngine のリファクタリング
- 全ての状態遷移にガードを実装。
- `StopReplay`: `RUNNING` / `PAUSED` のみ受理。確実に `IDLE` へ戻してから応答を返す。
- `ForceStopReplay`: 状態に関わらず Runner を停止し、即座に `IDLE` 復帰させてからレスポンスを返す。

## 3. gRPC 通信仕様の更新 (Adapted for Unary)

### 3.1 レスポンス形式の強化
`e-station` の `EngineBusy` 情報を Unary レスポンスに統合します。

```proto
message ReplayControlResponse {
  bool success = 1;
  string request_id = 2;
  // EngineBusy 相当の情報
  EngineState current_state = 3;
  string error_code = 4;
  string error_message = 5;
}
```

### 3.2 `engine.proto` (Payload Aligned)
`e-station` のフィールド番号を尊重し、末尾に `token` を追加する構成を徹底します。

```proto
service DataEngine {
  // 既存
  rpc GetState(GetStateRequest) returns (GetStateResponse);
  rpc Start(StartRequest) returns (StartResponse); // Legacy/Static Engine Start
  rpc Stop(StopRequest) returns (StopResponse);   // Legacy/Static Engine Stop

  // 新規追加 (Nautilus Replay)
  rpc LoadReplayData(LoadReplayDataRequest) returns (ReplayControlResponse);
  rpc StartEngine(StartEngineRequest) returns (ReplayControlResponse);
  rpc StopEngine(StopEngineRequest) returns (ReplayControlResponse);
  rpc SetReplaySpeed(SetReplaySpeedRequest) returns (ReplayControlResponse);
  rpc PauseReplay(PauseReplayRequest) returns (ReplayControlResponse);
  rpc ResumeReplay(ResumeReplayRequest) returns (ReplayControlResponse);
  rpc StepReplay(StepReplayRequest) returns (ReplayControlResponse);
  rpc StopReplay(StopReplayRequest) returns (ReplayControlResponse);
  rpc ForceStopReplay(ForceStopReplayRequest) returns (ReplayControlResponse);
}

message EngineStartConfig {
  string instrument_id = 1;
  repeated string instrument_ids = 2;
  optional string start_date = 3;
  optional string end_date = 4;
  optional string initial_cash = 5;
  optional ReplayGranularity granularity = 6;
  optional string strategy_file = 7;
  optional string strategy_init_kwargs = 8;
  optional uint32 max_qty = 9;
  optional uint64 max_notional_jpy = 10;
}

message StartEngineRequest {
  string request_id = 1;
  EngineKind engine = 2;
  string strategy_id = 3;
  EngineStartConfig config = 4;
  string token = 10;
}

message SetReplaySpeedRequest {
  string request_id = 1;
  uint32 multiplier = 2;
  string token = 10;
}
```

## 4. 実装ステップ

### 4.1 Step 1: `PAUSED` 状態の定義と基盤
- `engine.proto`, `schemas.py`, Rust Enum 全てに `PAUSED` を追加。
- `jquants_loader.py` 移植と `base_dir` 指定の徹底。

### 4.2 Step 2: gRPC 制御 API の実装
- `StartEngine`, `StopEngine`, `SetReplaySpeed` を含む全コマンドの実装。
- `e-station` 由来の `request_id` 紐付けと、`ReplayControlResponse` への状態埋め込み。

### 4.3 Step 3: レースコンディションと順序の保証
- `StopReplay` が確実に `IDLE` に戻してから応答を返すことを保証。
- Reducer において `TimeUpdated` -> `DataUpdated` の順序で `TradingState` を更新。

## 5. 検証（テスト計画）

### 5.1 状態遷移マトリクステスト
- **LoadReplayData**: 成功時に `IDLE -> LOADED`、失敗時に `IDLE` 維持。
- **StartEngine**: `LOADED` 以外では reject。成功時に `RUNNING`。
- **PauseReplay**: `RUNNING -> PAUSED`。それ以外は reject。
- **StepReplay**: `PAUSED` のみ受理。1件進行後も `PAUSED` を維持。
- **ResumeReplay**: `PAUSED -> RUNNING`。
- **StopReplay**: `RUNNING`/`PAUSED` のみ受理。応答前に `IDLE` 復帰。
- **ForceStopReplay**: 全状態から `IDLE` 復帰。

### 5.2 整合性・互換性テスト
- **GetState 互換性**: Phase 5 の `price/history/timestamp/timestamp_ms/history_points` 契約が Nautilus モードでも維持されること。
- **Reducer 堅牢性**: `ReplayTimeUpdated`、`KlineUpdate`、`Trades` の順序が入れ替わっても、最終的な `timestamp_ms` と `history_points` が破綻しないこと。
- **Deterministic Step**: 1件ずつのデータ進行と `timestamp_ms` の同期。
- **Auth/Token Test**: Unary 各リクエストのトークン検証。
- **ForceStop Resilience**: どのような状態からも `IDLE` に復旧できることの確認。

## 6. 注意点

- **J-Quants Path**: `S:/j-quants` は使用せず、テスト環境でも明示的なパス指定を行う。
- **Schema Consistency**: `schemas.py` の `ReplayStateName` に `PAUSED` が欠落していた `e-station` の不備を本プロジェクトでは確実に解消する。
