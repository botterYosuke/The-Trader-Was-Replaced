# Transparent Headless Replay Plan

[Tranceparent Python Backend](./Tranceparent%20Python%20Backend.md) の Phase 6 以降を具体化し、`nautilus_trader` を基盤とした本格的な headless replay engine の構築と、その後の実取引（live）連携を目指す計画です。

## Summary

- `nautilus_trader` の Replay Slice を移植し、Live 依存を完全に排除した `replay_runner.py` として再構成する。
- `e-station` 準拠の Replay State Machine を導入し、制御 API の安全性と整合性を確保する。
- 既存の Unary `GetState` 契約を維持しつつ、Nautilus のイベントを `TradingState` へ畳み込む「Snapshot Reducer」を実装する。
- リプレイを先行させ、Engine 内のイベント（Kline, Trade, Time）が正しく発火することを headless 環境で先に確認する。

## Phased Implementation

Phase 5 の chart / history / timestamp 契約は [Tranceparent Python Backend](./Tranceparent%20Python%20Backend.md) に統合済み。この計画では、その後続となる headless replay engine と live 連携を扱う。

### Phase 6: Nautilus Replay Slice & Engine Control

- **Goal**: Nautilus のリプレイ機能のみを抽出し、Live 依存のない自律的な Engine を構築する。
- **Tasks**:
  - **Replay Runner の分離移植**: `engine_runner.py` から replay streaming のみを抽出し、`replay_runner.py` として新設。
  - **Replay State Machine の導入**: `IDLE`, `LOADED`, `RUNNING`, `PAUSED`, `STOPPING` の状態管理と遷移ガードを実装。
  - **Engine 制御 API (MVP subset)**: `e-station` の制御命令のうち `StepBackward` を除くサブセットを実装。
    - `LoadReplayData`, `StartEngine` (戦略開始), `StopEngine` (戦略停止), `PauseReplay`, `ResumeReplay`, `SetReplaySpeed`, `StepReplay`
    - `StopReplay` / `ForceStopReplay` (リプレイセッション自体の停止/強制停止)
  - **Snapshot Reducer の実装**: Nautilus イベントを以下規則で `TradingState` へ変換。
    - `KlineUpdate`: `close` を `price` に、`open_time_ms` を `timestamp` に。
    - `Trades`: 直近の trade price を `price` に、`ts_ms` を `timestamp` に。
  - **Streaming 評価**: Unary 維持か `Session` streaming 移行かを決定。
- **Success Criteria**:
  - headless backend 上でリプレイデータがロードされ、制御命令に従って時刻と価格が進行すること。

### Phase 7: Replay UI Integration & Advanced Control

- **Goal**: Bevy UI で replay の進行状況や取引結果を可視化する。
- **Tasks**:
  - **Replay Time Sync**: Python 側の `replay_time` を購読。
    - Phase 6 で決定した通信方式に従い、polling または event stream subscribe で取得。
  - **Portfolio State の分離**: `Position` / `Order` サマリーを取得する専用 API/DTO を追加。
  - **Step-back 対応 (Optional)**: `StepBackward` API と snapshot ring buffer の導入。
- **Success Criteria**:
  - リプレイの進行に合わせてチャートとポートフォリオ情報が同期すること。

### Phase 8: Live Venue & Market Data

- **Goal**: 実取引環境への接続準備とマーケットデータの取得。
- **Tasks**:
  - **Venue Login**: Tachibana / Kabu-station への認証。Replay モード時は拒否。
  - **Ticker Metadata**: 銘柄情報の取得と `Instrument` への変換。
  - **Live Market Data**: `price`, `trades`, `depth` の購読。
- **Success Criteria**:
  - 実環境のライブ価格が Bevy UI 上に反映されること。

### Phase 9: Live Account & Order API

- **Goal**: 口座情報の同期と注文機能の実装。
- **Tasks**:
  - **Account Sync**: 口座残高や保有ポジションの同期。
  - **Order Entry**: 注文執行 API の移植。
- **Success Criteria**:
  - UI からの注文が可能になり、実環境の口座状態と一致すること。

### Phase 10: Replay-to-Live Strategy Execution

- **Goal**: リプレイで作成・検証したストラテジーを、コード変更なしでそのままライブ環境で実行できるようにする。
- **Tasks**:
  - **Strategy Portability**: Replay と Live で同一の `Strategy` 定義 (Nautilus `Strategy` サブクラス) を共有する仕組みを確立。
    - `replay_runner.py` と `live_runner.py` (Phase 8/9 で構築) の双方が、同じストラテジーモジュールをロードできるエントリポイントを提供。
    - Replay/Live 固有の依存（時刻ソース、データソース、Venue ID 等）はストラテジー外部から注入し、ストラテジー本体は環境非依存に保つ。
  - **Promote to Live API**: 現在 Strategy Editor で編集中のストラテジーを「ライブ実行」する制御 API を追加。
    - `StartLiveStrategy(strategy_id, instrument_id, venue, params)` / `StopLiveStrategy(run_id)`。
    - Replay の `StartEngine` と対称な State Machine を Live 側に持たせる。
  - **Run Mode Switch (UI)**: Strategy Editor / Replay UI に Live 切替を追加。
    - 明示的な確認ダイアログ（実発注の警告）と、Replay 結果サマリー（バックテスト KPI）を併記。
    - Replay 中の run と Live run を `run_id` で並列管理し、UI で区別表示。
  - **Safety Rails**: 誤発注防止のためのガード。
    - Venue 未ログイン / Replay モード中 / 口座未同期時は Live 起動を拒否。
    - 1 ストラテジーあたりの同時 Live インスタンスは 1 つに制限。
    - Position size / 注文金額の事前上限チェック（ユーザ設定可能）。
  - **Live Run Telemetry**: Live 実行中のイベント（fills, position, PnL）を Replay と同じ `TradingState` reducer 経由で UI に流す。
- **Success Criteria**:
  - Replay で動作確認したストラテジーを、ファイル編集なしに「Promote to Live」操作のみで実環境にデプロイできる。
  - Live 実行中もチャート・ポートフォリオ・注文履歴が Replay と同等の粒度で UI に反映される。
  - Safety Rails により、未認証 / モード不一致時の誤発注が構造的に防止される。

## Strategy & Principles

1. **Strict Dependency Isolation**: `replay_runner.py` で Live 依存を構造的に排除。
2. **State-Driven Integrity**: Replay State Machine による API 呼び出しの安全性確保。
3. **Compatibility through Reducer**: 既存 Unary `GetState` を維持しつつ、Nautilus イベントを `price/history/timestamp` へマッピング。
4. **Subscription Agnostic**: UI 側の同期ロジックは、backend の通信方式（Polling/Streaming）の変更を許容する設計とする。
