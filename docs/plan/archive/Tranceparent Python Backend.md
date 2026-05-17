# Python Backend Migration Plan

[e-station/python](C:\Users\sasai\Documents\e-station\python) で持っていた Python 実装を、このリポジトリの `python/` ディレクトリへ移植するための段階的な計画です。  
最初のゴールは UI 連携ではなく、Rust 側から独立して動く headless な Python バックエンドを先に用意することです。

## Summary

- Python 実装を `The-Trader-Was-Replaced/python` に新設する。
- Phase 1 では、gRPC で起動できる headless バックエンドを用意した。
- その後、Rust 側は Python バックエンドの gRPC API を読む構成に寄せていく。
- 最後に、replay / snapshot / chart 用データの供給を Python 側へ段階的に移す。

## Key Changes

- `python/` を新しい Python 実装のルートにする。
- まずは headless 起動と gRPC API を優先し、UI は後回しにする。
- backend の責務を次の順で移す。
  - 基本のマーケットデータ
  - replay / session / buffer
  - データ変換と永続化
  - Rust から読むチャート用 gRPC API
- Rust 側は当面、既存の擬似価格生成を残しつつ、Python backend 接続の土台を作る。
- その後、`src/trading.rs` の責務を Python 由来のデータに置き換える。

## Phased Implementation

### Phase 1: headless Python backend の最小起動

- Status: 完了。
- `python/` 配下に最小の Python backend を作る。
- CLI で headless モードを起動できるようにする。
- gRPC transport で起動し、起動時に次を確認できるようにする。
  - 設定読み込み
  - モデル初期化
  - 簡単な健康状態レスポンス
  - 固定の sample state 返却
- この段階では Rust 連携やライブ更新はまだ入れない。

#### Test

- backend が CLI で起動できる
- gRPC health endpoint が応答する
- sample state が返る

### Phase 2: データモデル移植

- Status: 完了（詳細は [Phase 2 - Data Model Migration.md](./Phase%202%20-%20Data%20Model%20Migration.md) を参照）。
- Rust 側の `TradingData` と整合する最小データモデルを Python に置く。
- `pydantic` で `TradingState` などの明示的な schema を定義し、backend の外部契約を固定する。

### Phase 3: replay / snapshot 対応

- Status: 完了（詳細は [Phase 3 - Replay and Snapshot.md](./Phase%203%20-%20Replay%20and%20Snapshot.md) を参照）。
- headless backend が replay データから state を生成できるようにする。
- snapshot の読み込みだけでも state を復元できるようにする。

### Phase 4: Rust 接続

- Status: 完了（詳細は [Phase 4 - Rust Integration.md](./Phase%204%20-%20Rust%20Integration.md) を参照）。
- Rust 側に backend 接続層を追加する。
- `tonic` + `tokio` による非同期通信を実装し、Bevy の描画ループをブロックしないようにする。
- 認証 token、port、接続設定を Rust 側の Resource として管理する。
- 既存の Rust 側シミュレーションはトグル可能にし、Python backend データへの切り替えをサポートする。

#### Test

- Rust が Python backend の gRPC API に非同期で接続できる
- price / history を取得して UI がスムーズに更新される
- backend 停止時や認証失敗時に Rust 側が適切にハンドルできる
- 設定によりシミュレーションとバックエンドを切り替えられる

### Phase 5: chart 用データ供給の強化 / Chart & History Contract Alignment

- Status: 実装計画策定済み（詳細は [Phase 5 - Chart Data and Enhanced Backend.md](./Phase%205%20-%20Chart%20Data%20and%20Enhanced%20Backend.md) を参照）。
- **Goal**: Bevy chart と backend history の契約を固め、Rust 側でも replay / history の時刻を Unix milliseconds ベースで扱えるようにする。
- Phase 4 の gRPC `GetState` 接続を前提に、まずは snapshot / poll ベースのまま chart 用データを強化する。
- 着手前に `C:\Users\sasai\Documents\e-station` を詳細に確認し、chart / backend 設定 / timestamp の扱いをこのリポジトリへ移植する。
  - chart 表示: `e-station/src/chart.rs`, `e-station/src/chart/kline.rs`, `e-station/src/widget/chart.rs`, `e-station/src/screen/dashboard/pane.rs`
  - chart データ構造: `e-station/data/src/chart.rs`, `e-station/data/src/aggr/time.rs`, `e-station/data/src/aggr/ticks.rs`, `e-station/data/src/chart/kline.rs`
  - backend wire 契約: `e-station/python/engine/schemas.py`, `e-station/python/engine/server_grpc.py`, `e-station/engine-client/src/dto.rs`
  - replay 時刻・進行: `e-station/python/engine/nautilus/engine_runner.py`, `e-station/python/engine/server.py`, `e-station/python/engine/replay_session.py`
- Bevy 側に chart system を追加する。最初は e-station の全機能を一度に持ち込まず、`TradingData` から描ける軽量な line chart / price chart を先に移植する。
  - `e-station` の `KlineChart` が持つ責務のうち、データ保持、表示範囲、autoscale、latest price line、cache invalidation の考え方を Bevy の system / component へ写す。
  - `e-station` の `ViewState` 相当として、表示範囲、価格レンジ、最大描画点数、hover/crosshair 用の状態を Bevy Resource または Component に分離する。
  - 将来の candlestick / footprint / heatmap 移植に備え、描画用データは単なる `Vec<f32>` だけに閉じない。
- backend 側で history の保持件数、間引き、更新頻度を設定できるようにする。
  - 現在の固定上限 `1000` と固定 advance interval `1.0s` を、CLI または env で変更可能にする。
  - 候補: `--max-history-len`, `--chart-history-len`, `--history-sample-stride`, `--advance-interval-sec`。
  - 長時間実行や replay の進行で history が肥大化しないよう、backend 側で上限と変換責務を持つ。
- Rust UI 側の chart 表示に必要な最大件数を決め、backend がその要求を満たすようにする。
  - Bevy 側は表示に必要な点数だけを保持し、描画時に毎フレーム大きな `Vec` を作り直さない。
  - backend から受け取る配列が過大な場合でも、Rust 側で最後の防衛として上限をかける。
- timestamp の扱いを e-station と揃える。
  - e-station では chart / replay / wire の主時刻は Unix milliseconds (`ts_ms`, `open_time_ms`, `timestamp_ms`)。
  - Phase 5 では Rust 側にも最新 `timestamp_ms` を保持し、chart の x 軸は index ではなく timestamp ベースで扱える形にする。
  - 現行 `TradingState.timestamp` は秒 `float` のまま後方互換として残しつつ、chart 用には `timestamp_ms` または `history_points` を追加する。
  - `history: Vec<f32>` は Phase 4 互換のため維持する。新しい chart 用データは additive な JSON フィールドとして追加し、古い Rust 側が壊れないようにする。
  - 候補データ形:
    - `history_points: [{ "timestamp_ms": 1700000000000, "price": 120.5 }]`
    - または `history_timestamps_ms: [1700000000000], history: [120.5]`
- `GetState` の JSON 契約は後方互換を維持し、`price` / `history` / `timestamp` は Phase 4 と同じ意味で残す。
- stream 化はこの段階では必須にせず、poll ベースで負荷や表示品質に問題が出た場合の後続対応として判断する。
- e-station の `KlineUpdate` / `Trades` / `ReplayTimeUpdated` のような push 型イベントは、この Phase では設計を参照するだけに留める。gRPC stream へ切り替える場合は Phase 6 以降の候補にする。

#### Test

- `e-station` の該当実装を確認し、移植対象と非移植対象を Phase 5 の詳細計画に列挙できている
- 長時間実行で state が壊れず、history が上限内に保たれる
- chart 表示に必要な件数の history が取得できる
- history の保持件数、間引き、更新頻度の設定変更で backend が壊れない
- `GetState` の JSON 契約が Phase 4 と互換のまま維持される
- Rust UI が取得した `price` / `history` / `timestamp` / chart 用 timestamp データで Bevy chart を更新できる
- Rust 側で `timestamp_ms` が保持され、replay の進行時刻と chart の x 軸がずれない
- backend から過大な history が返っても、Rust 側の chart system が上限内で安定して描画できる

## Test Plan

- 各 phase ごとに 1 つ以上の smoke test を置く。
- 最低限、次を通す。
  1. Python backend 単体起動
  2. Python の replay / snapshot 生成
  3. Rust からの gRPC 接続
  4. Rust UI での表示更新
- 追加で次の失敗系も確認する。
  - backend 起動失敗
  - 不正な設定
  - 壊れた JSON
  - backend 切断

## Assumptions

- live trading は当面扱わず、headless replay / snapshot を先に固める。
- UI 全面移植は後回しにし、まずは chart に必要な backend データだけを優先する。
- Rust 側の既存シミュレーションは、一旦残して段階的に Python へ寄せる。
- 通信は Phase 1 で入れた gRPC ベースで進める。
- Python API の外部契約は `price` / `history` / `timestamp` を中心にし、Rust 側固有の `Timer` などは backend schema に持ち込まない。
