# Implementation Plan: Phase 1 - Headless Python Backend (gRPC)

`docs/plan/Tranceparent Python Backend.md` の Phase 1 「headless backend の最小起動」を達成するための具体的な実装計画です。e-station の既存設計（gRPC）に準拠した構成へ修正しました。

## 1. 目的
- e-station 由来の Python 実装をベースに、GUI 依存を排除した headless backend を構築する。
- Rust (Bevy) 側から gRPC を介して接続可能な最小限のエンジンを起動する。
- 起動・設定・状態確認を CLI から行えるようにする。

## 2. 技術スタック
- **言語**: Python 3.11+
- **通信プロトコル**: gRPC (`grpcio`, `grpcio-tools`, `protobuf`)
- **データエンジン**: Nautilus Trader (e-station での使用に準拠)
- **CLI**: Typer
- **データバリデーション/シリアライズ**: Pydantic, orjson
- **パッケージ管理**: uv (e-station の `uv.lock` に準拠)

## 3. ディレクトリ構成案
プロジェクトルートに `python/` ディレクトリを作成し、e-station の構造を参考にしつつ整理します。

```text
python/
├── proto/
│   └── engine.proto      # e-station からコピー/調整
├── src/
│   ├── engine/           # e-station のコアロジックを移植
│   │   ├── __init__.py
│   │   ├── server.py     # gRPC サーバー実装
│   │   └── core.py       # Nautilus Trader 等の初期化
│   └── cli/
│       ├── __init__.py
│       └── main.py       # Typer による CLI 実装
├── tests/                # ユニットテスト・結合テスト
├── .python-version
├── pyproject.toml        # uv で管理
└── run.py                # エントリポイント
```

## 4. 実装ステップ

### Step 1: Python プロジェクトのセットアップ
1. `python/` ディレクトリの作成。
2. `uv init` によるプロジェクト初期化。
3. `grpcio`, `grpcio-tools`, `nautilus-trader`, `typer` 等の依存関係追加。

### Step 2: Protocol Buffers の導入
1. `e-station/proto/engine.proto` から必要な定義を `python/proto/` にコピー。
2. `grpc_tools.protoc` を使用して Python 用のコードを生成。

### Step 3: 最小限の gRPC サーバー実装
1. `src/engine/server.py` を作成。
2. `Health` チェック用のサービスを実装し、エンジンのステータスを返せるようにする。
3. 最初は固定の `TradingData` 相当を返すサービスを実装。

### Step 4: CLI の実装
1. `run.py` または `src/cli/main.py` でサーバーを起動・停止するコマンドを実装。
2. 設定ファイル（.env 等）を読み込み、ポート番号などを指定可能にする。

### Step 5: テストの実装
1. Python 単体で gRPC クライアントからサーバーを叩き、期待したレスポンスが返るか確認。
2. Headless モードでの正常起動・終了をテスト。

## 5. 動作確認項目
- [ ] `python run.py serve` で gRPC サーバーが起動する。
- [ ] gRPC クライアント（またはテストスクリプト）から接続し、正常応答が返る。
- [ ] Nautilus Trader のエンジンが初期化され、エラーなく動作する。
- [ ] CLI からログレベルやポートの設定が反映される。

## 6. 次のフェーズへの橋渡し
Phase 1 完了後、Phase 2 では Rust (Bevy) 側に `tonic` 等を導入して gRPC クライアントを構築し、Python 側から送られるリアルタイムデータを描画に反映させます。
