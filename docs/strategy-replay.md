# Strategy Replay

`python -m engine.strategy_replay` で Nautilus `Strategy` サブクラスを ParquetDataCatalog 上でリプレイし、run-buffer (`meta.json` / `fills.jsonl` / `equity.jsonl` / `summary.json`) を出力する。

## Quick start — `scripts/run_replay.ps1` ラッパー（推奨）

戦略ファイルを渡すだけで「SCENARIO 読取 → catalog 自動構築 → リプレイ実行」をワンショットで行う。`.env` の `DEV_J_QUANTS_CACHE`（既定: `S:/j-quants`）を J-Quants CSV のソースとして使用する。

```powershell
.\scripts\run_replay.ps1 -Strategy python\tests\data\test_strategy_daily.py
```

主なオプション：

| Flag | 用途 |
|---|---|
| `-Strategy <path>` | 戦略 `.py`（必須） |
| `-Catalog <dir>` | catalog ディレクトリ。既定: `artifacts/jquants-catalog` |
| `-RunBufferDir <dir>` | run-buffer 出力先。既定: `%APPDATA%\flowsurface\run-buffer\` |
| `-StrategyParam key=value` | 戦略 kwarg 上書き（繰り返し可） |
| `-SkipCatalogBuild` | catalog 自動構築をスキップ |
| `-VerboseRun` | DEBUG ログ有効化 |

## 手動実行

### 1. ParquetDataCatalog を構築

J-Quants CSV キャッシュ (`S:/j-quants`) から、戦略 SCENARIO の `instrument` / `start` / `end` / `granularity` をカバーする catalog を作成する。

```powershell
cd python
uv run python -c "from engine.jquants_to_catalog import ensure_jquants_catalog; ensure_jquants_catalog(base_dir='S:/j-quants', catalog_path='../artifacts/jquants-catalog', instrument_id='1301.TSE', start_date='2025-01-06', end_date='2025-03-31', granularity='Daily')"
```

### 2. 戦略をリプレイ

`python/` ディレクトリ直下で:

```powershell
uv run python -m engine.strategy_replay run `
    --strategy tests/data/test_strategy_daily.py `
    --catalog ../artifacts/jquants-catalog `
    --run-buffer-dir ../tmp/run-buffer
```

stdout に `run_id` / `run_dir` / `equity_points` / `fills_count` などの summary JSON が出力される。
`--run-buffer-dir` を省略すると `%APPDATA%\flowsurface\run-buffer\` に書かれる（blacksheep `ingest_run.py` 互換）。

### CLI オプション一覧

| Flag | 用途 |
|---|---|
| `--strategy PATH` | 戦略 `.py`（`SCENARIO` と Strategy サブクラスを含む） |
| `--catalog DIR` | ParquetDataCatalog のディレクトリ |
| `--bars-json FILE` | catalog の代わりに JSON 合成 Bar を使う（オフラインテスト用） |
| `--start / --end` | SCENARIO の期間を上書き（sweep スクリプト用） |
| `--granularity` | `Daily` / `Minute` の上書き |
| `--strategy-param KEY=VALUE` | 戦略の `__init__` kwarg を上書き（繰り返し可） |
| `--verbose` | DEBUG ログを有効化 |

## サンプル戦略

- [`python/tests/data/test_strategy_daily.py`](../python/tests/data/test_strategy_daily.py) — 1301.TSE / Daily / Buy-and-hold
- [`python/tests/data/test_strategy_minute.py`](../python/tests/data/test_strategy_minute.py) — Minute 版
- [`python/tests/data/test_strategy_trade.py`](../python/tests/data/test_strategy_trade.py) — Trade 版

## Bevy GUI でのリプレイ実行

ヘッドレス CLI ではなく、Bevy アプリ（`backcast.exe`）上で戦略を Run する手順。

### 前提条件

| 項目 | 値 |
|---|---|
| backend デフォルトポート | `19876` |
| 認証トークン | `BACKEND_TOKEN=testtoken`（`.env` 参照） |
| catalog パス | `artifacts\jquants-catalog`（`BACKEND_CATALOG_PATH` 参照） |

### 1. Backend 起動

```powershell
cd "C:\Users\sasai\Documents\The-Trader-Was-Replaced"
# ポート競合チェック（必要なら先に kill）
$p = (Get-NetTCPConnection -LocalPort 19876 -ErrorAction SilentlyContinue).OwningProcess
if ($p) { Stop-Process -Id $p -Force }

# backend 起動（新しい cmd ウィンドウで）
Start-Process cmd -ArgumentList "/k", "uv run python -m engine --token testtoken --jquants-catalog-path artifacts\jquants-catalog"
```

`Starting gRPC server on port 19876` が出れば OK。

> **注意**: `python -m engine.server_grpc` ではなく `python -m engine` を使うこと。`server_grpc` には `__main__` がないためエラーになる。

### 2. Rust アプリ起動

`.env` の値は自動読み込みされない。環境変数を **明示的に渡して** 起動すること。

```powershell
cd "C:\Users\sasai\Documents\The-Trader-Was-Replaced"

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = ".\target\debug\backcast.exe"
$psi.WorkingDirectory = $PWD.Path
$psi.UseShellExecute = $false
$psi.EnvironmentVariables["BACKEND_ENABLED"] = "true"
$psi.EnvironmentVariables["BACKEND_TOKEN"] = "testtoken"
$psi.EnvironmentVariables["BACKEND_CATALOG_PATH"] = "artifacts\jquants-catalog"
[System.Diagnostics.Process]::Start($psi) | Out-Null
```

> `cargo run` 単体や `Start-Process` 単体では `.env` が読まれず `grpc: DISABLED` になる。`ProcessStartInfo.EnvironmentVariables` で直接渡すのが確実。

### 3. 正常起動の確認

フッター（画面右下）に以下が表示されれば接続成功：

```
state: IDLE  grpc: OK
```

`grpc: DISABLED` が続く場合は `BACKEND_ENABLED=true` が渡っていない。`grpc: OK` でも `state: RUNNING` になる場合は backend の `auto_start` が `True` になっている（`python/engine/__main__.py` の `auto_start=False` を確認）。

### 4. 戦略の実行

1. メニューバー **`Open Strategy...`** → `.py` ファイルを選択
2. Strategy Editor ウィンドウが開く → **`Run`** をクリック
3. フッター `state: RUNNING` → 完了後 `IDLE` に戻る
4. Run Result Panel が `Completed` になり、fills / pnl が表示される
5. チャートエリアに最新バーの **candle（赤/緑）** が表示される

### トラブルシューティング

| 症状 | 原因 | 対処 |
|---|---|---|
| `grpc: DISABLED` | `BACKEND_ENABLED` 未設定 | `ProcessStartInfo` で明示的に渡す |
| Run ボタンが反応しない | `grpc: DISABLED` のまま | backend 起動 → Rust アプリ再起動 |
| 起動直後から `state: RUNNING` | `auto_start=True` になっている | `python/engine/__main__.py` を `auto_start=False` に修正 |
| candle が表示されない | `open_time_ms` が backend から届いていない | `python/engine/core.py` の `KlineUpdate` に `open_time_ms=ts_ms` があるか確認 |

---

## 詳細仕様

[docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md)
