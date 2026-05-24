# トラブルシューティング

「設定したはずなのに動かない」「ログにこのメッセージが出る」を起点に逆引きできるページです。

関連ページ: [getting-started](getting-started.md) / [settings](settings.md) / [venues](venues.md)

> 文中の `[G3]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

## 逆引き表

| 症状 | 原因 | 対処 |
|---|---|---|
| フッターが `grpc: DISABLED` のまま | `BACKEND_ENABLED` が GUI プロセスに渡っていない | `.env` は GUI に自動ロードされない。`ProcessStartInfo.EnvironmentVariables` 等で `BACKEND_ENABLED=true` を明示注入する [G3] |
| フッターの ▶ ボタンが半透明 / 反応しない | `cache_path` 未設定、または `grpc: DISABLED` | Strategy Editor で cache を保存 → 必要ならバックエンド起動 → GUI 再起動 [J7]/[J8]/[G3] |
| 起動直後から `state: RUNNING` になる | バックエンドの `auto_start=True` | `python/engine/__main__.py` が `auto_start=False` で起動しているか確認 |
| チャートに candle が表示されない | `open_time_ms` がバックエンドから届いていない | `KlineUpdate` に `open_time_ms` が含まれているか確認 [K1] |
| catalog が見つからない | catalog 未構築、または `DEV_J_QUANTS_CACHE` 不正 | `ensure_jquants_catalog` で構築。`DEV_J_QUANTS_CACHE`（既定 `S:/j-quants`）と `ARTIFACTS_PATH` を確認 [L6] |
| Run すると Run Result に `Catalog precision mismatch ... PRECISION_BYTES=16` が出る | catalog は standard-precision（8-byte）で書かれているのに、この PC の nautilus が high-precision（16-byte）ビルド。共有 catalog は書き換えない（Windows と衝突） | nautilus を standard-precision で再ビルドして揃える: `scripts/rebuild_nautilus_standard.sh`（`HIGH_PRECISION=false` で sdist から再ビルド → `PRECISION_BYTES=8`）。Intel Mac は PyPI に standard wheel が無く sdist 由来の high-precision ビルドを引きやすいので、`uv sync` 後はこのスクリプトを再実行する [A12] |
| venue ログインに失敗する | 認証情報・環境設定の誤り | Tachibana は `DEV_TACHIBANA_USER_ID` / `DEV_TACHIBANA_PASSWORD`、kabu は `DEV_KABU_API_PASSWORD` と kabuステーション本体の起動を確認 [D3] |
| Prod に接続できない / 送信が遮断される | 本番ガード | Tachibana は `TACHIBANA_ALLOW_PROD=1`、kabu は `KABU_ALLOW_PROD=1` を設定（二重ガード）[D8]/[L3] |
| ポート 19876 が競合して起動できない | 既存プロセスがポートを占有 | 既存プロセスを停止する（下記参照） [L5] |
| Connect メニュー項目が無効 / 非表示 | venue が未接続でない（接続処理中・接続中・再接続中・**ERROR** を含む）、または当該 venue が `--live-venue` で配線されていない | 先に Disconnect する（ERROR で固まったときも一度 Disconnect）。配線されていない venue は起動時の `--live-venue` を確認 |

## ポート 19876 の競合解消（PowerShell）

```powershell
$p = (Get-NetTCPConnection -LocalPort 19876 -ErrorAction SilentlyContinue).OwningProcess
if ($p) { Stop-Process -Id $p -Force }
```

## 既知の制約

- **Linux**: メニューバーの描画に既知の制約があります。
- **macOS**: Cmd+Q（アプリ終了）まわりに既知の制約があります。

## 一次情報

Replay まわりの起動・前提・トラブルシュートは `docs/strategy-replay.md` を一次情報として参照してください。
