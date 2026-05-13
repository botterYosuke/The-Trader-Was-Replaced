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
- 移行中の MVP では `SimpleCSVProvider` を Nautilus/J-Quants loader の代替 provider として扱う。`LoadReplayData` は replay provider が構成済みの場合のみ成功し、provider が存在しない static mode では `success=false` / `INVALID_STATE` 相当のエラーを返して `IDLE` を維持する。
- Legacy `Start` / `Stop` は既存互換の静的・簡易進行 API として残す。一方、Phase 6 の `LoadReplayData` / `StartEngine` / `PauseReplay` などは replay session 専用 API として扱い、static mode の代替起動経路にはしない。

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

### 4.1 Step 1: `PAUSED` 状態の定義と基盤 — **Completed**

- `engine.proto`, `schemas.py` 全てに `PAUSED` を追加済み。
- Replay state machine (`IDLE / LOADED / RUNNING / PAUSED / STOPPING`) 実装済み。
- `LoadReplayData`, `StartEngine`, `PauseReplay`, `StepReplay`, `ResumeReplay`, `StopReplay`, `ForceStopReplay` を実装し、provider 必須ガード確認済み。
- `jquants_loader.py` 移植と `base_dir` 指定の徹底済み。
- fast / slow テスト分離済み。

### 4.2 Step 2: gRPC 制御 API の実装 — **Completed**

- `StartEngine`, `StopEngine`, `SetReplaySpeed`, `PauseReplay`, `ResumeReplay`, `StepReplay`, `StopReplay`, `ForceStopReplay` 全コマンド実装済み。
- `request_id` 紐付けと `ReplayControlResponse.current_state` 返却済み。
- Unary gRPC 経由で `DAILY` / `MINUTE` が実データ価格まで進行することを確認済み。
- `StepReplay` は `PAUSED` のみ受理し、1 tick 進行後も `PAUSED` を維持する。

### 4.3 Step 3: レースコンディションと順序の保証 — **Completed for MVP**

- `StopReplay` が確実に `IDLE` に戻してから応答を返すことを保証済み。
- `ReplayTimeUpdated -> KlineUpdate` の順序発火を `NautilusReplayRunner` で強制。
- `ReducerState` を導入し、stale タイムスタンプを無視、同一タイムスタンプは後着優先で処理。
- `event_log` を追加し、順序検証テストで利用。
- `prime` は `event_log` に載せない規則を確立。

## 5. 検証（テスト計画）

### 5.1 状態遷移マトリクステスト
- **LoadReplayData**: 成功時に `IDLE -> LOADED`、失敗時に `IDLE` 維持。
- **LoadReplayData without provider**: replay provider が未構成の場合は reject し、`IDLE` を維持。
- **StartEngine**: `LOADED` 以外では reject。成功時に `RUNNING`。
- **PauseReplay**: `RUNNING -> PAUSED`。それ以外は reject。
- **StepReplay**: `PAUSED` のみ受理。1件進行後も `PAUSED` を維持。
- **ResumeReplay**: `PAUSED -> RUNNING`。
- **StopReplay**: `RUNNING`/`PAUSED` のみ受理。応答前に `IDLE` 復帰。
- **ForceStopReplay**: 全状態から `IDLE` 復帰。

### 5.2 整合性・互換性テスト — **MVP Completed**

- **GetState 互換性**: Phase 5 の `price/history/timestamp/timestamp_ms/history_points` 契約が Nautilus モードでも維持されること。✅
- **OHLC fields**: `open/high/low/close` を reducer と `GetState` に追加済み。✅
- **Reducer 堅牢性**: `ReplayTimeUpdated -> KlineUpdate` 順序の強制、stale event 無視を確認済み。✅
- **Deterministic Step**: `timestamp_ms` / `history_points` の同期テスト通過済み。✅
- **Auth/Token Test**: Unary 各リクエストのトークン検証済み。✅
- **ForceStop Resilience**: 全状態から `IDLE` 復帰を確認済み。✅
- **Nautilus adapter**: `Bar` / `TradeTick` → reducer event 変換を `nautilus_adapter.py` に実装済み。✅
- **Nautilus catalog loader**: `ParquetDataCatalog.query(data_cls=Bar, identifiers=[bar_type])` 経由のローダーを `nautilus_catalog_loader.py` に実装済み。✅
- **J-Quants → catalog 変換**: `jquants_to_catalog.py` の `convert_daily_to_catalog` / `convert_minute_to_catalog` / `ensure_jquants_catalog` 実装済み。✅
- **DataEngine catalog route 完全置換**: `JQuantsDailyReplayProvider` / `JQuantsMinuteReplayProvider` を廃止し、全 J-Quants replay が catalog route を通ることを確認済み。✅
- **gRPC catalog ingestion**: `LoadReplayData(catalog_path=..., instrument_ids=[bar_type])` で Nautilus catalog から replay 可能。✅
- **Trade granularity**: Phase 6 MVP では意図的に対象外とし、`TICK` granularity は reject する。✅

## 6. 注意点

- **J-Quants Path**: `S:/j-quants` は使用せず、テスト環境でも明示的なパス指定を行う。
- **Schema Consistency**: `schemas.py` の `ReplayStateName` に `PAUSED` が欠落していた `e-station` の不備を本プロジェクトでは確実に解消する。
- **catalog path**: `jquants_loader` が設定されている場合、`jquants_catalog_path` は必須。未設定なら `"J-Quants catalog path is not configured"` で fail する。
- **catalog query**: `catalog.query(data_cls=Bar, identifiers=[str(bar_type)], start=None, end=None)`。日付フィルタは CSV 変換時点で実施済みなので query には渡さない。
- **ns → ms 変換**: `Bar.ts_event` は nanoseconds。`nautilus_adapter.py` 内で `// 1_000_000` して ms に変換する。他の場所では変換しない。
- **prime は event_log に載せない**: prime 時の tick は reducer 初期状態の構築に使うが、`event_log` への追記は行わない。

## 7. Phase 6 MVP 到達状況

### 現在の正式データ経路

```text
CLI / env (--jquants-catalog-path / JQUANTS_CATALOG_PATH)
-> python -m engine
-> serve(jquants_dir=..., jquants_catalog_path=...)
-> DataEngine(jquants_loader=..., jquants_catalog_path=...)
-> gRPC LoadReplayData(DAILY/MINUTE, instrument_ids=[bar_type])
-> DataEngine.load_replay_data()
-> ensure_jquants_catalog()       # CSV -> ParquetDataCatalog
-> NautilusBarsReplayProvider     # catalog から Bar を読み込み
-> prime / StepReplay
-> ReplayTimeUpdated + KlineUpdate
-> ReducerState
-> GetState
```

### Remaining / Next

- **Trade granularity の判断**: `TICK` をいつ、どのような形でサポートするか決定する。現時点では reject。
- **Optional**: `event_log` を外部に公開する gRPC ストリーム / ポーリング API の追加。
- **Optional**: catalog replay を本物の `BacktestDataEngine` / `LiveDataEngine` に置き換え（Phase 7 以降）。
