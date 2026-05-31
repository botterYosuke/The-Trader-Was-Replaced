# 設定と環境変数

> 文中の `[M21]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

関連ページ: [getting-started](getting-started.md) / [venues](venues.md) / [troubleshooting](troubleshooting.md)

## Help→Settings modal

メニューバー **Help(&H)** → **Settings** を選択すると、Settings modal が表示されます（`src/ui/settings.rs`）。 [M24]

- `Alt+H` でも Help メニューを開閉できます。
- modal は backdrop + 320px カードの UI Node 構成で、canvas には乗りません。
- **× ボタン** または **Escape キー** で閉じられます。 [M24]
- 既に開いている場合は重複して開きません。 [M24]
- modal はオンデマンド spawn / despawn のため、セーブ・ロード・再起動では復元されません。 [M24]

| 項目 | 値 |
|---|---|
| Theme | Dark [M24] |
| Backend | localhost:19876 [M24] |
| Save Layout | — [M24] |

## バックエンド（in-proc）

- Python エンジンは Rust バイナリに **PyO3 で同一プロセスに埋め込まれています**（in-proc）。別プロセスのバックエンドを起動する必要はありません（旧 gRPC バックエンドは #64 / #68 で撤去済み）。
- ライブ venue は `LIVE_VENUE` 環境変数（`TACHIBANA` / `KABU`）で選択します。未設定だと Replay 専用で起動します。
- 起動は `run_inproc.ps1` 1 本で完結します。詳細は [getting-started](getting-started.md) / ルート README.md §起動方法 を参照してください。

## 環境変数

> **注意**: `run_inproc.ps1` で起動すると、スクリプトが in-proc 起動に必要な環境変数（`BACKEND_TRANSPORT=inproc` / `BACKEND_ENABLED` 等）を設定します。手動起動する場合は環境変数を明示的に設定してください。詳細は [getting-started](getting-started.md) / [troubleshooting](troubleshooting.md) を参照してください。

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
| `BACKEND_ENABLED` | `true` で in-proc バックエンド接続を有効化（未設定だと footer `backend: DISABLED`） |
| `LIVE_VENUE` | ライブ venue の選択（`TACHIBANA` / `KABU`）。未設定だと Replay 専用 |
| `BACKEND_TOKEN` | バックエンド認証トークン（例: `testtoken`） |
| `FLOWSURFACE_ENGINE_TOKEN` | エンジン接続用トークン |
| `STRATEGY_PARAM_*` | 戦略パラメータの上書き（例: `STRATEGY_PARAM_HOLDING_MINUTES=42` → 戦略の `holding_minutes`） |

> `.env` に置かないもの: 立花の**第二暗証番号**は環境変数に保存せず、ログイン時にプロンプトで入力します（[venues](venues.md) 参照）。

## データ準備

J-Quants の CSV から Nautilus ParquetDataCatalog への変換は自動で行われます。 [L6]

- ソース: `DEV_J_QUANTS_CACHE`（既定 `S:/j-quants`）の J-Quants CSV
- 出力: `{ARTIFACTS_PATH}/jquants-catalog`（`--jquants-catalog-path` / `JQUANTS_CATALOG_PATH` で明示指定も可能）
- ベース `base_dir` は `DEV_J_QUANTS_CACHE` を既定とします。

詳細な手順は [getting-started](getting-started.md) と `docs/strategy-replay.md` を参照してください。
