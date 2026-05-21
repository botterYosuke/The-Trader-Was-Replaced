# はじめに

このページでは、前提環境の準備からバックエンドと GUI の起動、最初の Replay 実行までの最短手順を説明します。GUI 上での実行手順の一次情報は `docs/strategy-replay.md` です。

## 前提

| 項目 | 内容 |
|---|---|
| Python | 3.10 以上 |
| uv | Python 依存の取得・実行に使用（`uv sync`） |
| cargo | Rust GUI のビルド・起動に使用 |
| catalog | ParquetDataCatalog。既定では `{ARTIFACTS_PATH}/jquants-catalog` を参照する |

> **注意**: `.env` は GUI（`backcast.exe`）から自動ロードされません。GUI を起動する際は、環境変数（`BACKEND_ENABLED`, `BACKEND_TOKEN`, `ARTIFACTS_PATH`）を明示的に注入する必要があります。

Python 依存は次のように取得します。

```bash
cd python
uv sync
```

## 1. バックエンド（Python gRPC）を起動

バックエンドは GUI とは別プロセスです。`python/` ディレクトリで起動します。

```bash
cd python && uv run python -m engine --token your-secret-token
```

主な引数は以下のとおりです。

| 引数 | 既定値 | 説明 |
|---|---|---|
| `--token` | （必須） | 認証トークン |
| `--port` | `19876` | gRPC ポート |
| `--transport` | `grpc` | トランスポート |
| `--max-history-len` | `1000` | 履歴の保持上限 |
| `--advance-interval-sec` | `1.0` | リプレイの進行間隔（秒） |
| `--jquants-catalog-path` | env `JQUANTS_CATALOG_PATH` / `ARTIFACTS_PATH` | catalog パス |
| `--live-venue` | None（Replay のみ） | `TACHIBANA` / `KABU` |

`Starting gRPC server on port 19876` が表示されれば起動成功です。

> `python -m engine.server_grpc` ではなく `python -m engine` を使ってください（`server_grpc` には `__main__` がないためエラーになります）。

リプレイ実行時は、catalog を含む環境変数を渡して起動するのが確実です（`docs/strategy-replay.md` の手順）。

```powershell
# 既存プロセスがポートを掴んでいたら停止
$p = (Get-NetTCPConnection -LocalPort 19876 -ErrorAction SilentlyContinue).OwningProcess
if ($p) { Stop-Process -Id $p -Force }

# backend 起動（新しい cmd ウィンドウで）
Start-Process cmd -ArgumentList "/k", "uv run python -m engine --token testtoken --jquants-catalog-path artifacts\jquants-catalog"
```

## 2. GUI（Rust / Bevy）を起動

`.env` は自動読み込みされないため、環境変数を明示注入して起動します。`docs/strategy-replay.md` の PowerShell 手順（`ProcessStartInfo.EnvironmentVariables`）が一次情報です。

```powershell
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = ".\target\debug\backcast.exe"
$psi.WorkingDirectory = $PWD.Path
$psi.UseShellExecute = $false
$psi.EnvironmentVariables["BACKEND_ENABLED"] = "true"
$psi.EnvironmentVariables["BACKEND_TOKEN"] = "testtoken"
$psi.EnvironmentVariables["ARTIFACTS_PATH"] = $PWD.Path + "\artifacts"
[System.Diagnostics.Process]::Start($psi) | Out-Null
```

> `cargo run` 単体や `Start-Process` 単体では `.env` が読まれず `grpc: DISABLED` になります。`ProcessStartInfo.EnvironmentVariables` で直接渡すのが確実です。
> `ARTIFACTS_PATH` は catalog のベースディレクトリで、GUI が `{ARTIFACTS_PATH}/jquants-catalog` を参照します。省略するとリポジトリ直下の `artifacts/` が既定になります。

事前に debug ビルドを作るには `cargo build`、開発時に env を注入しない簡易起動なら `cargo run` を使えますが、後者はバックエンドに接続せず `grpc: DISABLED` になります。

## 3. 接続を確認

GUI 画面右下のフッターに次が表示されれば接続成功です。

```
state: IDLE  grpc: OK
```

`grpc: DISABLED` が続く場合は `BACKEND_ENABLED=true` が渡っていません。

> backend 接続状態（`grpc: OK`）は [G1]、切断後の自己修復（再接続で復帰）は [G2]、`BACKEND_ENABLED=false` 時の `grpc: DISABLED`（と replay clock 非反映）は [G3] で保証されます。flow 一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

## 4. 最初の Replay 実行

1. メニューバー左の **File(&F)** から **Open (Ctrl+O)** で戦略の **サイドカー JSON（`<strategy>.json`）** を選択する（ファイルダイアログは `.json` のみを表示する。同じ場所の `<strategy>.py` が自動で読み込まれる）
2. Strategy Editor ウィンドウが開く
3. フッター中央の **▶** ボタンをクリックして Run を開始 [A1]
4. フッターが `state: RUNNING` になり、ボタンが **||**（一時停止／再開）に切り替わる [A2]
5. 完了すると `state: IDLE` に戻り、Run Result パネルが `Completed` になり fills / pnl が表示される [A1]/[B1]
6. チャートエリアに最新バーのローソク足（赤／緑）が描画される

詳細な手順は [Replay 実行](replay.md) を参照してください。

## 画面の見取り図

![画面構成](assets/screen-layout.drawio.svg)

| エリア | 位置 | 役割 |
|---|---|---|
| メニューバー | 上 | File / Edit / Venue メニュー |
| サイドバー | 左 | 銘柄リスト＋価格、パネルを開くボタン、Settings |
| フッター | 下 | 実行モードトグル、再生コントロール、速度、Venue 状態、gRPC 状態 |
| フローティングウィンドウ | 中央 | Strategy Editor / Chart / Buying Power / Positions / Orders / Run Result |

各エリアの詳細は [画面構成](screen-layout.md) を参照してください。
