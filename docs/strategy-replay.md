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

## 詳細仕様

[docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md)
