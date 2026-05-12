# Python Backend Migration Plan

`e-station/python` で持っていた Python 実装を、このリポジトリの `python/` ディレクトリへ移植するための段階的な計画です。  
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

- Status: 計画済み（詳細は [Phase 4 - Rust Integration.md](./Phase%204%20-%20Rust%20Integration.md) を参照）。
- Rust 側に backend 接続層を追加する。
- `tonic` + `tokio` による非同期通信を実装し、Bevy の描画ループをブロックしないようにする。
- 認証 token、port、接続設定を Rust 側の Resource として管理する。
- 既存の Rust 側シミュレーションはトグル可能にし、Python backend データへの切り替えをサポートする。

#### Test

- Rust が Python backend の gRPC API に非同期で接続できる
- price / history を取得して UI がスムーズに更新される
- backend 停止時や認証失敗時に Rust 側が適切にハンドルできる
- 設定によりシミュレーションとバックエンドを切り替えられる

### Phase 5: chart 用データ供給の強化

- Phase 4 の gRPC `GetState` 接続を前提に、まずは snapshot / poll ベースのまま chart 用データを強化する。
- backend 側で history の保持件数、間引き、更新頻度を設定できるようにする。
- Rust UI 側の chart 表示に必要な最大件数を決め、`TradingState.history` がその要求を満たすようにする。
- `GetState` の JSON 契約は維持し、`price` / `history` / `timestamp` を中心にした headless でも再利用できるデータ形に保つ。
- 長時間実行や replay の進行で history が肥大化しないよう、backend 側で上限と変換責務を持つ。
- stream 化はこの段階では必須にせず、poll ベースで負荷や表示品質に問題が出た場合の後続対応として判断する。

#### Test

- 長時間実行で state が壊れず、history が上限内に保たれる
- chart 表示に必要な件数の history が取得できる
- history の保持件数、間引き、更新頻度の設定変更で backend が壊れない
- `GetState` の JSON 契約が Phase 4 と互換のまま維持される
- Rust UI が取得した `price` / `history` / `timestamp` で chart を更新できる

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
