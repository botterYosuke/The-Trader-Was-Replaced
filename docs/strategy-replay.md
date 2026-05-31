# Strategy Replay

`python -m engine.strategy_replay` で Nautilus `Strategy` サブクラスを ParquetDataCatalog 上でリプレイし、run-buffer (`meta.json` / `fills.jsonl` / `equity.jsonl` / `summary.json`) を出力する。

## Quick start — `scripts/run_replay.ps1` ラッパー（推奨）

戦略ファイルを渡すだけで「scenario 読取 → catalog 自動構築 → リプレイ実行」をワンショットで行う。`.env` の `DEV_J_QUANTS_CACHE`（既定: `S:/j-quants`）を J-Quants CSV のソースとして使用する。

```powershell
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
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

J-Quants CSV キャッシュ (`S:/j-quants`) から、戦略 scenario の `instrument` / `start` / `end` / `granularity` をカバーする catalog を作成する。

```powershell
cd python
uv run python -c "from engine.jquants_to_catalog import ensure_jquants_catalog; ensure_jquants_catalog(base_dir='S:/j-quants', catalog_path='../artifacts/jquants-catalog', instrument_id='1301.TSE', start_date='2025-01-06', end_date='2025-03-31', granularity='Daily')"
```

### 2. 戦略をリプレイ

`python/` ディレクトリ直下で:

```powershell
uv run python -m engine.strategy_replay run `
    --strategy ../examples/test_strategy_daily.py `
    --catalog ../artifacts/jquants-catalog `
    --run-buffer-dir ../tmp/run-buffer
```

stdout に `run_id` / `run_dir` / `equity_points` / `fills_count` などの summary JSON が出力される。
`--run-buffer-dir` を省略すると `%APPDATA%\flowsurface\run-buffer\` に書かれる（blacksheep `ingest_run.py` 互換）。

### CLI オプション一覧

| Flag | 用途 |
|---|---|
| `--strategy PATH` | 戦略 `.py`（Strategy サブクラスを含む。SCENARIO は `<strategy>.json` の `scenario` キーに書く） |
| `--catalog DIR` | ParquetDataCatalog のディレクトリ |
| `--bars-json FILE` | catalog の代わりに JSON 合成 Bar を使う（オフラインテスト用） |
| `--start / --end` | SCENARIO の期間を上書き（sweep スクリプト用） |
| `--granularity` | `Daily` / `Minute` の上書き |
| `--strategy-param KEY=VALUE` | 戦略の `__init__` kwarg を上書き（繰り返し可） |
| `--verbose` | DEBUG ログを有効化 |

## サンプル戦略

- [`examples/test_strategy_daily.py`](../examples/test_strategy_daily.py) — 1301.TSE / Daily / Buy-and-hold
- [`examples/test_strategy_minute.py`](../examples/test_strategy_minute.py) — Minute 版
- [`examples/test_strategy_trade.py`](../examples/test_strategy_trade.py) — Trade 版

> **SCENARIO の書き方**: 各戦略の SCENARIO は `.py` 内ではなく、同名の `<strategy>.json` の `scenario` キーに書く。例:
>
> ```json
> {
>   "scenario": {
>     "schema_version": 1,
>     "instrument": "1301.TSE",
>     "start": "2025-01-06",
>     "end": "2025-03-31",
>     "granularity": "Daily",
>     "initial_cash": 1000000
>   }
> }
> ```
>
> `.py` 内に `SCENARIO` が残っている外部戦略（本リポ外）は **Python CLI からのみ** legacy fallback で動く（WARN ログが出る）。GUI（Bevy）からは実行不可。

## Bevy GUI でのリプレイ実行

ヘッドレス CLI ではなく、Bevy アプリ（`backcast.exe`）上で戦略を Run する手順。

Python エンジンは Rust プロセスに **PyO3 で同一プロセスに埋め込まれている**（in-proc）ため、別プロセスのバックエンドを起動する必要はない。GUI の起動はラッパースクリプト 1 本で完結する。起動手順（`run_inproc.ps1` / ビルド前提 / 正常起動ログ）はルートの [README.md §起動方法](../README.md#起動方法) を一次情報として参照すること。

```powershell
.\scripts\run_inproc.ps1
# artifacts を別ドライブに置く場合:
.\scripts\run_inproc.ps1 -ArtifactsPath S:\artifacts
```

`run_inproc.ps1` が `BACKEND_TRANSPORT=inproc` / `BACKEND_ENABLED` 等の環境変数設定・`__pycache__` 削除・`backcast.exe` 起動を一括で行う。catalog は `{ARTIFACTS_PATH}/jquants-catalog`（`ARTIFACTS_PATH` env var から自動構築、デフォルト: `{cwd}/artifacts`）を参照する。

#### DLL パス設定

`backcast.exe` は PyO3 embedding により `python3.dll` にリンクしています。
uv 管理の Python を使っている場合、`python3.dll` は `Scripts/` ではなく **base Python ディレクトリ**（`uv python dir` で確認できる場所の直下）に置かれています。
Windows のローダーがこのディレクトリを探せないと `0xC0000135` でクラッシュします。 [P12]

解決策は 2 通りあります。どちらか一方で OK です。

**A. `PYTHON_DLL_DIR` 環境変数で指定する（推奨）**

次の行を `run_inproc.ps1` 起動前のシェルで設定してください:

```powershell
# uv が管理する Python の base dir を取得する例
$pyBase = Split-Path (uv run python -c "import sys; print(sys.executable)")
$env:PYTHON_DLL_DIR = $pyBase
```

**B. PATH に追加する**

```powershell
$pyBase = Split-Path (uv run python -c "import sys; print(sys.executable)")
$env:PATH = "$pyBase;" + $env:PATH
```

> `PYTHON_DLL_DIR` は `backcast.exe` 起動時に読み取り、DLL 検索パスに追加します。

### 3. 正常起動の確認

フッター（画面右下）に以下が表示されれば in-proc バックエンドへの接続成功：

```
state: IDLE  backend: OK
```

`backend: DISABLED` が続く場合は `BACKEND_ENABLED=true` が渡っていない（`run_inproc.ps1` で起動すればスクリプトが設定する）。

### 4. 戦略の実行

1. メニューバー **`Open Strategy...`** → `.py` ファイルを選択
2. Strategy Editor ウィンドウが開く
3. フッター中央の **`▶`** ボタン（PauseResume）をクリックして Run を開始
4. フッター `state: RUNNING` → ボタンが **`||`** に切り替わる（クリックで Pause、PAUSED 中は再度 **`▶`** で Resume）
5. 完了後 `state: IDLE` に戻り、Run Result Panel が `Completed` になり fills / pnl が表示される
6. チャートエリアに最新バーの **candle（赤/緑）** が表示される

### トラブルシューティング

| 症状 | 原因 | 対処 |
|---|---|---|
| `backend: DISABLED` | `BACKEND_ENABLED` 未設定 | `run_inproc.ps1` で起動する（スクリプトが設定）。手動起動時は `BACKEND_ENABLED=true` を設定 |
| フッターの ▶ ボタンが半透明 / 反応しない | `cache_path` 未設定、または `backend: DISABLED` | Strategy Editor で cache を保存 → `run_inproc.ps1` で再起動 |
| candle が表示されない | `open_time_ms` が backend から届いていない | `python/engine/core.py` の `KlineUpdate` に `open_time_ms=ts_ms` があるか確認 |
| 起動直後に `0xC0000135` でクラッシュ | `python3.dll` が見つからない | `PYTHON_DLL_DIR` 環境変数に Python base ディレクトリを設定するか、同ディレクトリを `PATH` に追加する（§ DLL パス設定 参照） |

---

## 詳細仕様

[docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md)
