# 設定と環境変数

関連ページ: [getting-started](getting-started.md) / [venues](venues.md) / [troubleshooting](troubleshooting.md)

## サイドバー Settings セクション

サイドバー下部の **Settings** セクションに現在の設定が表示されます（`src/ui/sidebar.rs`）。

| 項目 | 値 |
|---|---|
| Theme | Dark |
| Backend | localhost:19876 |
| Save Layout | — |

## ポートとバックエンド

- バックエンド gRPC サーバーの既定ポートは **19876** です。
- バックエンドは `python -m engine` で起動します（`python -m engine.server_grpc` は `__main__` が無いため不可）。
- ライブ venue を使う場合は起動時に `--live-venue TACHIBANA` または `--live-venue KABU` を付けます。省略すると Replay 専用で起動します。

## 環境変数

> **注意**: `.env` は Rust GUI に自動ロードされません。GUI を起動するプロセスから環境変数を明示的に注入する必要があります（`ProcessStartInfo.EnvironmentVariables` 等）。詳細は [getting-started](getting-started.md) / [troubleshooting](troubleshooting.md) を参照してください。

| 環境変数 | 用途 |
|---|---|
| `ARTIFACTS_PATH` | 成果物のベースディレクトリ。catalog は `{ARTIFACTS_PATH}/jquants-catalog` に構築される |
| `JQUANTS_CATALOG_PATH` | Nautilus ParquetDataCatalog のパス（`--jquants-catalog-path` の既定値） |
| `DEV_J_QUANTS_CACHE` | J-Quants CSV のソースディレクトリ（既定: `S:/j-quants`） |
| `DEV_TACHIBANA_USER_ID` | 立花証券 e支店のユーザー ID |
| `DEV_TACHIBANA_PASSWORD` | 立花証券 e支店のパスワード |
| `DEV_TACHIBANA_DEMO` | `true` で立花のデモ環境を使う |
| `DEV_KABU_API_PASSWORD` | kabuステーション API パスワード |
| `TACHIBANA_ALLOW_PROD` | `1` で立花の本番環境接続を許可（未設定だと Prod 接続を遮断） |
| `KABU_ALLOW_PROD` | `1` で kabu の本番環境 (localhost:18080) 接続を許可 |
| `TACHIBANA_SESSION_PATH` | 立花のセッションキャッシュ JSON のパス |
| `BACKEND_ENABLED` | `true` で GUI がバックエンド gRPC を有効化（未設定だと `grpc: DISABLED`） |
| `BACKEND_TOKEN` | バックエンド認証トークン（例: `testtoken`） |
| `FLOWSURFACE_ENGINE_TOKEN` | エンジン接続用トークン |
| `STRATEGY_PARAM_*` | 戦略パラメータの上書き（例: `STRATEGY_PARAM_HOLDING_MINUTES=42` → 戦略の `holding_minutes`） |

> `.env` に置かないもの: 立花の**第二暗証番号**は環境変数に保存せず、ログイン時にプロンプトで入力します（[venues](venues.md) 参照）。

## データ準備

J-Quants の CSV から Nautilus ParquetDataCatalog への変換は自動で行われます。

- ソース: `DEV_J_QUANTS_CACHE`（既定 `S:/j-quants`）の J-Quants CSV
- 出力: `{ARTIFACTS_PATH}/jquants-catalog`（`--jquants-catalog-path` / `JQUANTS_CATALOG_PATH` で明示指定も可能）
- ベース `base_dir` は `DEV_J_QUANTS_CACHE` を既定とします。

詳細な手順は [getting-started](getting-started.md) と `docs/strategy-replay.md` を参照してください。
