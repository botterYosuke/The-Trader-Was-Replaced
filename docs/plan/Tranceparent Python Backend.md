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

- Rust 側の `TradingData` と整合する最小データモデルを Python に置く。
- `pydantic` で `TradingState` などの明示的な schema を定義し、backend の外部契約を固定する。
- まずは次を定義する。
  - `price`
  - `history`
  - `timestamp`
- Rust 側の `timer` は Bevy 内部の更新制御なので、Python API の外部契約には出さない。
- sample state は dict の直書きではなく、schema を通した値として返す。
- 変換ロジックは UI ではなく backend の責務として持つ。

#### Test

- schema の妥当性を検証できる
- sample state が schema validation を通る
- 既存の sample JSON が正しい形で読める
- Rust 側が期待する最小項目を出せる
- 不正な price / history / timestamp を拒否できる

### Phase 3: replay / snapshot 対応

- headless backend が replay データから state を生成できるようにする。
- snapshot の読み込みだけでも state を復元できるようにする。
- 復元結果は Phase 2 の `TradingState` schema を必ず通す。
- この段階では live trading は扱わず、決定論的な入力だけを使う。

#### Test

- replay データから同じ state が再現される
- snapshot の読み込みが成功する
- 異常時に backend が落ちず、エラーを返す

### Phase 4: Rust 接続

- Rust 側に backend 接続層を追加する。
- まずは poll ベースの gRPC `GetState` 経由でつなぐ。
- 認証 token、port、backend 起動失敗時の扱いを Rust 側の設定に入れる。
- Rust の既存シミュレーションは、この段階で Python backend データに置き換える。

#### Test

- Rust が Python backend の gRPC API に接続できる
- price / history を取得して画面更新できる
- backend 停止時に Rust が適切に失敗する
- token 不一致時に Rust 側が認証失敗として扱える

### Phase 5: chart 用データ供給の強化

- chart 表示に必要な更新頻度とバッファを backend 側で扱う。
- 必要なら stream 化するが、最初は snapshot ベースを優先する。
- headless でも再利用できるデータ形に保つ。

#### Test

- 長時間実行で state が壊れない
- history が十分に保持される
- 更新頻度の変更で backend が壊れない

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
