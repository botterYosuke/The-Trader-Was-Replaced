# Implementation Plan: Phase 1 - Headless Python Backend (gRPC)

`docs/plan/Tranceparent Python Backend.md` の Phase 1 「headless backend の最小起動」を達成するための具体的な実装計画です。e-station の実体構造（`python -m engine` 起動および `server_grpc.py`）に準拠した構成へ修正しました。

## 1. 目的
- e-station 由来の Python 実装をベースに、GUI 依存を排除した headless backend を構築する。
- Rust (Bevy) 側から gRPC を介して接続可能な最小限のエンジンを起動する。
- e-station と同一の起動インターフェース（`python -m engine`）を確保する。

## 2. 技術スタック
- **言語**: Python 3.11+
- **通信プロトコル**: gRPC (`grpcio`, `grpcio-tools`, `protobuf`)
- **データエンジン**: Nautilus Trader (e-station での使用に準拠)
- **CLI**: argparse (e-station の `__main__.py` に準拠)
- **データバリデーション/シリアライズ**: Pydantic, orjson
- **パッケージ管理**: uv (e-station の `uv.lock` に準拠)

## 3. ディレクトリ構造案
プロジェクトルートに `python/` ディレクトリを作成し、e-station の構造を模倣します。

```text
python/
├── engine/               # `python -m engine` で起動可能なパッケージ構造
│   ├── __init__.py
│   ├── __main__.py       # エントリポイント (argparse によるフラグ処理)
│   ├── server_grpc.py    # gRPC サーバー実装 (GrpcDataEngineServer)
│   ├── server.py         # その他トランスポート（WebSocket等）用
│   ├── core.py           # エンジン初期化ロジック
│   └── proto/            # 生成された pb2 コードの配置先
├── proto/
│   └── engine.proto      # e-station からコピーした原典
├── tests/                # ユニットテスト
├── .python-version
└── pyproject.toml        # uv で管理
```

## 4. 実装ステップ

### Step 1: Python プロジェクトのセットアップ
1. `python/` ディレクトリの作成。
2. `uv init` によるプロジェクト初期化。
3. `grpcio`, `grpcio-tools`, `nautilus-trader` 等の依存関係追加。

### Step 2: Protocol Buffers の導入
1. `e-station/proto/engine.proto` から定義を `python/proto/` にコピー。
2. `grpc_tools.protoc` を使用して Python 用েরコードを生成し、`engine/proto/` へ配置。

### Step 3: gRPC サーバーとエントリポイントの実装
1. `engine/server_grpc.py` を作成し、`GrpcDataEngineServer` クラスを実装。
2. `engine/__main__.py` を作成し、`argparse` を用いて以下のフラグを処理する。
   - `--port`: 待ち受けポート
   - `--token`: 認証用トークン
   - `--transport`: プロトコル選択（デフォルト: `grpc`）
3. `python -m engine --port 19876 --token dev-token` で起動可能にする。

### Step 4: 最小限の service 実装
1. `Health` チェックサービスを実装。
2. 最初は固定の `TradingData` を返すサービスを実装。

### Step 5: テストの実装と検証
1. `tests/` に pytest を用いた自動テストを実装。
2. 下記の「5. テスト仕様」に基づき、正常系・異常系のカバレッジを確保。
3. `python -m engine` 形式での起動から終了までの一連の挙動を検証。

## 5. テスト仕様 (Phase 1)

実装した各機能が正しく動作することを以下の項目で検証します。

| カテゴリ | 検証項目 | 期待される結果 |
| :--- | :--- | :--- |
| **CLI / 起動** | 正常起動（フル引数） | `python -m engine --port 19876 --token dev-token` でプロセスが維持される |
| | 引数欠如 | `--port` または `--token` がない場合、エラーを出力して終了する |
| | 不正なトランスポート | `--transport invalid` でエラーメッセージが表示される |
| **gRPC 通信** | ヘルスチェック | `Health.Check` サービスが `SERVING` (または OK 相当) を返す |
| | サンプルデータ取得 | `GetState` 等で `data/sample_state.json` と一致する決定的なデータが返る |
| **認証 / セキュリティ** | トークン一致 | 正しいトークンでのリクエストが受理される |
| | トークン不一致 | 誤ったトークンでの接続が `UNAUTHENTICATED` エラーで拒絶される |
| **ログ / 観測性** | 起動時ログ | 標準エラー出力に `port`, `transport`, `backend mode` (headless) が明記される |
| | 接続ログ | gRPC クライアントからの接続・切断がログに記録される |

## 6. 動作確認（Smoke Test）
開発の最終段階で、以下の代表的なケースを 1 本の smoke test として実行します。
- **ケース**: `python -m engine` を立ち上げ、別プロセスのテストスクリプトから正しいトークンで `GetState` を呼び出し、期待した価格データが 1 件以上取得できること。

## 7. 次のフェーズへの橋渡し
Phase 1 完了後、Phase 2 では Rust (Bevy) 側からこの gRPC サーバーへ接続し、データの取得を開始します。
