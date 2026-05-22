# バックテスト（ヘッドレス CLI）

> 文中の `[L1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

GUI を起動せず、コマンドラインだけで戦略を過去データ上でリプレイし、結果（約定・エクイティ・サマリ）をファイルに書き出す方法。CI への組み込みやパラメータスイープに向く。

GUI でチャートを見ながら再生したい場合は [replay.md](replay.md) を参照。

## 推奨: ラッパースクリプト

`scripts/run_replay.ps1` は「シナリオ読取 → catalog 自動構築 → リプレイ実行」をワンショットで行う。 [L1]/[L6]

```powershell
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

動作の流れ:

1. 戦略ファイルからシナリオを読み取る（サイドカー `<strategy>.json` の `scenario`、AST ベースで副作用なし）。 [L1]
2. catalog に必要な instrument / 期間 / 粒度の Bar が無ければ `ensure_jquants_catalog` で自動構築する（J-Quants CSV のソースは `DEV_J_QUANTS_CACHE`、既定 `S:/j-quants`）。 [L6]
3. `python -m engine.strategy_replay run` を実行する。 [L1]/[L2]

### オプション

| フラグ | 用途 |
|---|---|
| `-Strategy <path>` | 戦略 `.py`（必須） |
| `-Catalog <dir>` | catalog ディレクトリ。既定: `artifacts/jquants-catalog`（`ARTIFACTS_PATH` から自動構築） |
| `-RunBufferDir <dir>` | run-buffer 出力先。既定: `%APPDATA%\flowsurface\run-buffer\` |
| `-StrategyParam key=value` | 戦略の `__init__` kwarg を上書き（繰り返し可、カンマ区切りも可） |
| `-SkipCatalogBuild` | catalog 自動構築をスキップ |
| `-VerboseRun` | DEBUG ログを有効化 |

実行後、`run_id` / `run_dir` / `equity_points` / `fills_count` / `total_pnl` が表示される。 [L1]

## 直接 CLI

`python/` ディレクトリ直下で実行する。 [L2]

```powershell
uv run python -m engine.strategy_replay run `
    --strategy ../examples/test_strategy_daily.py `
    --catalog ../artifacts/jquants-catalog `
    --run-buffer-dir ../tmp/run-buffer
```

### 引数

| フラグ | 用途 |
|---|---|
| `--strategy PATH` | 戦略 `.py`（必須。Strategy サブクラスを含む。SCENARIO はサイドカー `<strategy>.json` の `scenario` キーから読む） |
| `--catalog DIR` | ParquetDataCatalog のディレクトリ（`--bars-json` を使わない場合は必須） |
| `--bars-json FILE` | catalog の代わりに JSON 合成 Bar を使う（オフラインテスト用） |
| `--run-buffer-dir DIR` | run-buffer 出力先を上書き（既定: `%APPDATA%\flowsurface\run-buffer\`） |
| `--strategy-param KEY=VALUE` | 戦略の `__init__` kwarg を上書き（繰り返し可） |
| `--granularity Daily\|Minute` | SCENARIO の粒度を上書き |
| `--start DATE` | SCENARIO の開始日を上書き（スイープ用） |
| `--end DATE` | SCENARIO の終了日を上書き（スイープ用） |
| `--verbose` / `-v` | DEBUG ログを有効化 |

`--catalog` と `--bars-json` のいずれかが必須。 [L2]

## 出力

stdout に summary JSON が出力される。 [L2]

```json
{
  "run_id": "...",
  "run_dir": "...",
  "equity_points": 60,
  "fills_count": 1,
  "total_pnl": "..."
}
```

run-buffer の既定出力先は `%APPDATA%\flowsurface\run-buffer\`（`--run-buffer-dir` / `-RunBufferDir` で変更可）。`run_dir` 以下に次のファイルが書かれる。 [L2]

| ファイル | 内容 |
|---|---|
| `meta.json` | run のメタ情報（戦略ファイル・シナリオ等） |
| `fills.jsonl` | 約定の行区切り JSON |
| `equity.jsonl` | エクイティ推移の行区切り JSON |
| `summary.json` | サマリ（stdout と同じ内容） |

## GUI Replay との違い

| 項目 | CLI（本ページ） | GUI Replay |
|---|---|---|
| 実行形態 | ヘッドレス・スタンドアロン | backend（gRPC）経由 |
| 可視化 | なし（JSON 出力のみ） | チャート・パネルで可視化 |
| 起動 | `run_replay.ps1` / `python -m engine.strategy_replay` | アプリのフッター ▶ ボタン |
| データ | catalog または `--bars-json` | backend が catalog から供給 |
| 用途 | 数値検証・CI・スイープ | 目視・体感確認 |

## サンプル戦略

リポジトリ内のサンプル（戦略 `.py` と同名 `.json` がペア）:

| ファイル | 内容 |
|---|---|
| `examples/test_strategy_daily.py` | 1301.TSE / Daily / バイアンドホールド |
| `examples/test_strategy_minute.py` | Minute 版 |
| `examples/test_strategy_7203_daily.py` | 7203.TSE / Daily |
| `examples/pair_trade_minute.py` | 2 銘柄（schema v2）/ Minute |

戦略の書き方・SCENARIO スキーマは [strategy.md](strategy.md) を参照。

## 関連ページ

- [replay.md](replay.md) — GUI での Replay 操作
- [strategy.md](strategy.md) — 戦略の書き方・SCENARIO
- [modes.md](modes.md) — 3 モードの概要
