# Phase 6.5: Blacksheep Strategy Runtime - Implementation Plan

## Context

Phase 6 MVP は `J-Quants CSV → ParquetDataCatalog → gRPC LoadReplayData → NautilusBarsReplayProvider → ReducerState → GetState` の経路を確立した。これは「市場データを time-stepping して UI が観測できる」段階で、**Nautilus `Strategy` のライフサイクル (cache.instrument / subscribe_bars / order_factory / submit_order / fills / equity)** を回す runtime はまだ存在しない。

一方、戦略研究リポジトリ `C:\Users\sasai\Documents\🐃_blacksheep` は、Nautilus `Strategy` subclass を `--strategy <path>` で動的ロードして replay 実行する仕組みを必要とし、現状は隣接リポジトリ `C:\Users\sasai\Documents\e-station` の `python -m engine.replay_session run` に依存している。`e-station` は `BacktestEngine` を直接駆動し、`%APPDATA%\flowsurface\run-buffer\{run_id}\` に `meta.json` / `fills.jsonl` / `equity.jsonl` を吐き、`blacksheep/scripts/ingest_run.py` がそれを Bronze/Silver/Wiki 層に流す。

Phase 6.5 のゴールは **`The-Trader-Was-Replaced` 単体で `blacksheep` の戦略 (`mean_reversion_01.py` / `order_flow_06.py`) を replay 実行し、`blacksheep` の ingest 契約 (schema_version=1) を一切壊さずに run-buffer を出力すること**。Phase 6 で構築した ParquetDataCatalog を `BacktestEngine.add_data()` への入力として再利用し、catalog ルートを「正式入口」として維持する。

## 0. 設計判断（決定事項）

| トピック | 決定 | 根拠 |
|---|---|---|
| データ供給 | **Catalog → BacktestEngine**。`ParquetDataCatalog.query(Bar, …)` で得た `list[Bar]` を `engine.add_data(bars)` に投入 | Phase 6 catalog 資産を活かす。`jquants_loader` の移植コストを払わない |
| run-buffer | `%APPDATA%\flowsurface\run-buffer\{run_id}\` （e-station と完全同居） | `blacksheep/scripts/ingest_run.py` が無改造で読める |
| schema | `meta.json`/`fills.jsonl`/`equity.jsonl` は **schema_version=1** を踏襲。`fills.jsonl` には `commission` 必須 (ingest_run の breakdown が依存) | 互換性最優先 |
| 起動経路 | 新規 CLI `python -m engine.strategy_replay run --strategy <path>`。`replay_session` 相当を本リポジトリ内に独立実装 | gRPC ルート (LoadReplayData) と並存。Phase 6.6 で gRPC `RunStrategyReplay` を追加予定 |
| MVP 範囲 | `mean_reversion_01.py` **と** `order_flow_06.py` の両方が通ること。後者は universe JSON / warmup_loader / STRATEGY_PARAM_* env / `C:\tmp\*_trades_*.jsonl` tee まで対応 | ユーザー要求 |

## 1. 関連ファイル早見表

### 1.1 本リポジトリ既存資産（再利用）
- [python/engine/nautilus_catalog_loader.py](../../python/engine/nautilus_catalog_loader.py) — `query_bars(catalog_path, bar_type, start, end)`
- [python/engine/jquants_to_catalog.py](../../python/engine/jquants_to_catalog.py) — catalog 構築 (既に Daily/Minute 両対応)
- [python/engine/core.py](../../python/engine/core.py) — `DataEngine` (Phase 6 reducer 経路。今回は触らない)
- [python/engine/replay.py](../../python/engine/replay.py) — `NautilusBarsReplayProvider` (今回は触らない)
- [python/proto/engine.proto](../../python/proto/engine.proto) — gRPC schema (今回は触らない)
- env: `JQUANTS_CATALOG_PATH`

### 1.2 e-station からの移植元 (`c:\Users\sasai\Documents\e-station`)
- `python/engine/replay_session.py` (L1357-2488) — CLI / run_id / RunBuffer tee
- `python/engine/nautilus/engine_runner.py` (L385-996) — `BacktestEngine` 初期化、Venue/Instrument 登録、msgbus subscribe、per-bar MTM
- `python/engine/run_buffer.py` (L44-328) — RunBuffer writer (path / schema)
- `python/engine/summary.py` (L69-155) — `compute_summary()`
- `python/engine/scenario.py` — `extract()` AST literal_eval / v1/v2/v3 resolve
- `python/engine/nautilus/strategy_loader.py` (L43-106) — `importlib.util` でファイル → モジュール → Strategy subclass 抽出

### 1.3 blacksheep 側契約 (`c:\Users\sasai\Documents\🐃_blacksheep`)
- `strategies/mean_reversion_01.py` — SCENARIO v1, `strategy_id="mean-reversion-01"`
- `strategies/order_flow_06.py` — SCENARIO v3, `UNIVERSE_JSON_PATH`, `warmup_loader` callback, `STRATEGY_PARAM_UNIVERSE_JSON_PATH` 等の env override、`C:\tmp` への `order_flow_06_trades_{utc_sec}.jsonl` 自己出力
- `scripts/ingest_run.py` — `meta.json`/`fills.jsonl`/`equity.jsonl` 必須、`C:\tmp\*_trades_*.jsonl` を時刻一致で自動収集、`engine.summary.compute_summary` を呼ぶ
- `python/run_buffer_path.py` — `%APPDATA%\flowsurface\run-buffer\<run_id>\` を解決

## 2. 新規モジュール構成

すべて本リポジトリの `python/engine/strategy_runtime/` に新設する（Phase 6 の `engine/` ルート資産と分離して影響を限定する）。

```
python/engine/strategy_runtime/
├── __init__.py
├── __main__.py            # `python -m engine.strategy_replay` の thin entry
├── cli.py                 # argparse + run() の orchestration
├── scenario.py            # e-station scenario.extract() の port (v1/v2/v3)
├── strategy_loader.py     # e-station strategy_loader.py の port
├── catalog_data_loader.py # **新規** Catalog → list[Bar] → engine.add_data()
├── engine_runner.py       # e-station engine_runner.py の port (catalog 化版)
├── run_buffer.py          # e-station run_buffer.py の port (schema=1 完全互換)
├── summary.py             # e-station summary.py の port
├── universe.py            # **新規** universe JSON loader (order_flow_06 用)
└── warmup.py              # **新規** catalog-backed warmup_loader (order_flow_06 用)
```

## 3. 実装フェーズ

### Step 1: scenario + strategy loader port  *(blocker)*

**ファイル**: `engine/strategy_runtime/scenario.py`, `engine/strategy_runtime/strategy_loader.py`

- `scenario.extract(path)`: AST `literal_eval` で `SCENARIO` 定数のみ抽出（モジュール実行禁止）。v3 の `instruments_ref` は同一フォルダ起点で JSON 解決 (e-station L2316-2410 と同等)。
- `strategy_loader.load(path) -> (module, scenario, strategy_cls)`:
  - `importlib.util.spec_from_file_location` でファイルを直接モジュール化
  - モジュール内で唯一の `nautilus_trader.trading.strategy.Strategy` サブクラスを抽出 (e-station L43-106)
  - `STRATEGY_PARAM_*` 環境変数を `StrategyConfig` フィールドに上書きする（order_flow_06 が依存）
- `_check_compat()`: 未実装フック（`on_order_book_delta` 等）への参照を warn。

**完了条件**: 単体テスト `test_scenario_extract.py` / `test_strategy_loader.py` が green。`mean_reversion_01.py` / `order_flow_06.py` の両方で `(module, scenario, cls)` を得られる。

### Step 2: catalog → Bars data loader  *(Phase 6 catalog 経路の最重要結合点)*

**ファイル**: `engine/strategy_runtime/catalog_data_loader.py`

- 既存 `python/engine/nautilus_catalog_loader.py::query_bars()` を活用。
- API: `load_bars_for_scenario(catalog_path, scenario) -> dict[InstrumentId, list[Bar]]`
  - 単一/複数 instrument 両対応 (SCENARIO v1/v2/v3)
  - granularity (`DAILY`/`MINUTE`) を Nautilus `BarType` 文字列 (`{symbol}.{venue}-1-{granularity}-LAST-EXTERNAL`) にマップ
  - 期間 (`start`/`end`) を catalog query に渡す（query 内部 filter は Phase 6 docs に従い無し → CSV 変換時点でフィルタ済み前提だが、念のため Python 側で `ts_event` フィルタを書く）
- 複数 instrument の場合は `sorted(all_bars, key=lambda b: b.ts_event)` で時系列に並べ、e-station と同じ streaming 順を再現。

**完了条件**: `test_catalog_data_loader.py` で「Phase 6 が生成した実 catalog (`JQUANTS_CATALOG_PATH`) から `1301.TSE` Minute の Bar 列が ts_event 昇順で取れる」を確認。

### Step 3: BacktestEngine runner port  *(中核)*

**ファイル**: `engine/strategy_runtime/engine_runner.py`

e-station `engine_runner.py` L385-996 を以下の差分で移植:

1. **データ注入を catalog ベースに置換**
   - jquants_loader の呼び出しを削除し、Step 2 の `load_bars_for_scenario()` に差し替える
   - streaming loop (`engine.add_data([item]) → engine.run(streaming=True) → engine.clear_data()`) は **そのまま維持** (per-bar pacing / 中断 / DateChangeMarker を温存)
2. **Venue / Instrument 登録**
   - e-station と同様 `Venue(safe_venue)` + `OmsType.NETTING` + `make_equity_instrument()` (J-Quants 銘柄 → Equity instrument)。helper を `engine/strategy_runtime/instrument_factory.py` として別ファイル化
3. **BacktestEngineConfig**
   - `cache_database_config=None` を維持（永続化禁止／e-station spec.md §3.2）
   - `trader_id = f"REPLAY-{strategy_id.safe()}"`
   - `bypass_logging` ON
4. **msgbus subscribe (event capture)**
   - `events.fills.{instrument_id}` → `OrderFilled` を **schema 3.21 ExecutionMarker** (`side`, `qty`, `price`, `commission`, `ts_event_ms`) に変換し、RunBuffer の `fills.jsonl` に push
   - `data.bars.{bar_type}` → per-bar MTM で `ReplayBuyingPower` (`equity`, `ts`) を計算し、RunBuffer の `equity.jsonl` に push
5. **clock / 営業日マーカー**
   - `compute_sleep_sec()` / `is_new_trading_day()` を移植
   - MVP では sleep は無効化（pacing は live 時のみ。replay は最高速）。実装は flag (`--pacing none|wallclock`) で切替
6. **GUI / WS attach mode は移植しない**
   - e-station の `mode=auto|attach|inprocess` のうち、Phase 6.5 は **inprocess のみ**実装。`--mode` は CLI に残すが `attach` は今回 reject。

**完了条件**: `engine_runner.run(strategy, scenario, run_buffer, catalog_path)` が、`mean_reversion_01.py` を 1 銘柄 Minute で完走させ、`fills.jsonl` / `equity.jsonl` に行が書かれる。

### Step 4: RunBuffer + Summary port  *(schema 互換性 critical)*

**ファイル**: `engine/strategy_runtime/run_buffer.py`, `engine/strategy_runtime/summary.py`

- e-station `run_buffer.py` を **schema_version=1 完全踏襲**で移植。
  - path: `_appdata_run_buffer_root()` を実装し `%APPDATA%\flowsurface\run-buffer\{run_id}\` を返す（Windows 以外は Phase 6.5 では対象外で OK）
  - `make_run_id()` = `{utc_sec}-{strategy_stem}-{instrument}` （e-station L57-61）
  - `meta.json` フィールド: `schema_version=1`, `run_id`, `strategy_file`, `strategy_sha256` (file の SHA256), `git_rev` (本リポジトリの HEAD)、`scenario`, `started_at`, `finished_at`, `status` (`finished`/`aborted`)
  - `fills.jsonl` event schema: e-station ExecutionMarker と完全一致 (PII scrub)
  - `equity.jsonl` event schema: e-station ReplayBuyingPower と完全一致
  - flush: 各 write_event 直後に `fh.flush()`、`finish()` で `os.fsync()`
  - **narrative.jsonl は MVP 範囲外** (空ファイルでも作成しない。ingest_run.py は narrative の不在を許容する設計)
- `summary.compute_summary(run_buffer_dir)`: e-station L69-155 をそのまま port。fields: `total_pnl`, `max_drawdown`, `trade_count`, `win_rate`, `fee_total`, `equity_points`, `fills_count`。**ただしこの関数を実際に呼ぶのは blacksheep ingest_run.py 側** (e-station への import 依存)。本リポジトリは Silver/summary.json を書かない。ingest_run が `engine.summary` をどこから import するかは Step 5 で対処する。

**完了条件**: `test_run_buffer_schema.py` で e-station 既存 run (fixture) と field set / 型 / 文字列フォーマットがバイト単位で一致。

### Step 5: blacksheep ingest 互換性  *(契約検証)*

`blacksheep/scripts/ingest_run.py` の依存:
1. **run-buffer path**: Step 4 で `%APPDATA%\flowsurface\run-buffer\<run_id>\` に出力済 → `ingest_run.py` のデフォルト path 解決 (`run_buffer_path.py` L18-26) に一致 ✓
2. **必須ファイル**: `meta.json` / `fills.jsonl` / `equity.jsonl` → Step 4 で全て生成 ✓
3. **`engine.summary` import**: blacksheep は `--e-station DIR` 引数 or `E_STATION_ROOT` env で path を解決 → **本リポジトリの `python/engine/strategy_runtime/summary.py` を同一インターフェースで提供**し、新規 env `THE_TRADER_ROOT` を `ingest_run.py` 側に追加してもらう…のではなく、**本リポジトリの `summary` モジュールを `engine.summary` という名前で sys.path 提供する shim** を `python/engine/__init__.py` 側に書く方が摩擦が少ない。決定: `python/engine/summary.py` という再エクスポート shim を 1 ファイル追加し、blacksheep からは `--e-station <path-to-The-Trader-Was-Replaced>` で本リポジトリを指せばよい状態にする。
4. **`C:\tmp\*_trades_*.jsonl` 自動検出**: order_flow_06 は戦略自身が `C:\tmp` に書き出すので、本リポジトリ側は何もしなくてよい ✓

**完了条件**: 以下を手動 (or integration test で) 実行し pass:
```powershell
# 本リポジトリで replay
uv run python -m engine.strategy_replay run `
  --strategy "C:\Users\sasai\Documents\🐃_blacksheep\strategies\mean_reversion_01.py" `
  --mode inprocess
# → %APPDATA%\flowsurface\run-buffer\<run_id>\ が出来る

# blacksheep で ingest
cd C:\Users\sasai\Documents\🐃_blacksheep
uv run python scripts/ingest_run.py <run_id> --e-station C:\Users\sasai\Documents\The-Trader-Was-Replaced
# → raw/replay-runs/<run_id>/, Silver/runs/<run_id>/summary.json, wiki/runs/<run_id>.md が生成
```

### Step 6: order_flow_06 対応  *(scope 拡張)*

`order_flow_06.py` 固有の依存:

1. **`UNIVERSE_JSON_PATH` (相対 path)**: 戦略ファイルディレクトリ起点で解決。`engine/strategy_runtime/universe.py` に `load_universe(json_path) -> UniverseV1` を実装。schema: `instruments[]`, `by_date{date: [instruments]}`, `scenario_start`, `scenario_end`。
2. **`STRATEGY_PARAM_UNIVERSE_JSON_PATH` 等の env override**: Step 1 の `strategy_loader.load()` で `STRATEGY_PARAM_*` を `StrategyConfig` に上書きする処理を実装済（再掲）。
3. **`warmup_loader: Callable[[symbol, start, end], list[tuple[date,o,h,l,c,v]]]`**: `engine/strategy_runtime/warmup.py` に **catalog-backed** な実装を新設。
   - 既存 `nautilus_catalog_loader.query_bars()` で DAILY Bar を読み、`(date, open, high, low, close, volume)` の tuple list に変換
   - 戦略は `strategy_init_kwargs={"warmup_loader": catalog_warmup_loader}` で受け取る
4. **複数銘柄注入**: Step 2 が複数 instrument を時系列マージするので順序問題は無い。`engine.add_instrument()` を universe `instruments[]` 全件に対し事前実行
5. **`C:\tmp\*_trades_*.jsonl` 自己出力**: 戦略内部の仕業。runner は触らない。`ingest_run.py` が時刻 prefix で自動拾い上げる
6. **戦略 init**: `mean_reversion_01` は `StrategyConfig` を `Strategy(config=cfg)` に渡すだけ。`order_flow_06` は class 直接 instantiate + 多数の `__init__` kwargs。`strategy_loader.load()` が両パターンをサポートする (e-station L43-106 と同じ二段構え：`config=` を受ける場合は config 渡し、それ以外は kwargs 渡し)

**完了条件**: `order_flow_06.py` を universe JSON 付き Minute で 1 営業日 replay し、`fills.jsonl` 行数 > 0、`C:\tmp\order_flow_06_trades_*.jsonl` が生成、`ingest_run.py` の Bronze/Silver/Wiki まで通る。

### Step 7: CLI 配管

**ファイル**: `engine/strategy_runtime/cli.py`, `engine/strategy_runtime/__main__.py`

- `argparse` 構造（e-station L2424-2488 を簡素化）:
  ```
  python -m engine.strategy_replay run \
    --strategy <path>                    # required
    [--catalog <path>]                   # default: $JQUANTS_CATALOG_PATH
    [--instrument <symbol>]              # SCENARIO override
    [--start <YYYY-MM-DD>] [--end ...]
    [--granularity DAILY|MINUTE]
    [--initial-cash <int>]
    [--mode inprocess]                   # only "inprocess" accepted in Phase 6.5
    [--pacing none|wallclock]            # default none
  ```
- SCENARIO の値を引数で上書き (CLI > SCENARIO の優先順位)
- run() の orchestration:
  1. `strategy_loader.load(path)` → `(module, scenario, cls)`
  2. CLI args で scenario 上書き → `validate()`
  3. `make_run_id(scenario, strategy_path)` → `RunBuffer(run_id)`
  4. `catalog_data_loader.load_bars_for_scenario(catalog, scenario)` → bars
  5. `engine_runner.run(cls, scenario, bars, run_buffer)` （内部で BacktestEngine 構築、msgbus subscribe、streaming loop）
  6. 例外時 `run_buffer.abort()`、正常時 `run_buffer.finish()`
  7. stdout に `run_id` と `%APPDATA%\flowsurface\run-buffer\<run_id>\` の絶対 path を出力

**完了条件**: `uv run python -m engine.strategy_replay run --help` がエラー無く出る。MVP コマンドが mean_reversion_01 で完走する。

### Step 8 (deferred / 非 Phase 6.5)

- gRPC `RunStrategyReplay` RPC の追加 (proto 拡張 + Rust client) → Phase 6.6
- UI からの起動 → Phase 6.6
- wallclock pacing / live attach mode → Phase 7
- Trade granularity (TICK) での戦略実行 → Phase 7

## 4. 非ゴール（明示）

- **Live trading**: Tachibana / Kabu API への発注は対象外。`BacktestEngine` の inprocess replay のみ。
- **W&B publish**: `🐃_blacksheep` 側 (`ingest_run.py` 以降) の責務として完全に維持。本リポジトリは run-buffer まで。
- **wiki / Silver の書き込み**: `blacksheep/scripts/ingest_run.py` の責務。本リポジトリは書かない。
- **Trade granularity (TICK)**: Phase 6 MVP 同様、現時点では catalog に TradeTick が無いため対象外。
- **gRPC / UI 統合**: Phase 6.5 では独立 CLI のみ。
- **catalog 自動構築**: 利用者は事前に `JQUANTS_CATALOG_PATH` を作っておく前提（Phase 6 と同じ運用）。

## 5. テスト計画

すべて `python/tests/strategy_runtime/` 配下に新設。pytest marker `slow` を実 catalog 依存テストに付与。

| Test | 種別 | 内容 | 完了条件 |
|---|---|---|---|
| `test_scenario_extract.py` | unit | v1/v2/v3 を AST literal_eval で抽出 | mean_reversion_01 / order_flow_06 の SCENARIO が dict として返る |
| `test_strategy_loader.py` | unit | importlib + Strategy subclass 抽出 + STRATEGY_PARAM_* override | env `STRATEGY_PARAM_HOLDING_MINUTES=42` で config に反映 |
| `test_catalog_data_loader.py` (slow) | integ | 実 catalog から `1301.TSE` Minute 取得 | ts_event 昇順、行数 > 0 |
| `test_run_buffer_schema.py` | unit | fixture (e-station 既存 run の copy) と field/型一致 | jsonschema validate pass |
| `test_summary_parity.py` | unit | 同じ run-buffer fixture に対し e-station の `compute_summary` と完全一致 | `assert summary == expected_summary` |
| `test_fake_strategy_e2e.py` | integ | `tests/fixtures/strategies/fake_buy_and_hold.py` を 5 bar 流す | fills>=1, equity>=5, status="finished" |
| `test_mean_reversion_smoke.py` (slow) | integ | mean_reversion_01 を 1 銘柄 1 日 Minute で完走 | run-buffer 3 ファイル生成、status="finished" |
| `test_blacksheep_ingest_compat.py` (slow, manual) | acceptance | 本リポジトリで生成した run-buffer を blacksheep `ingest_run.py` に食わせる | exit 0、`Silver/runs/<run_id>/summary.json` 生成 |
| `test_order_flow_06_smoke.py` (slow, optional) | integ | universe JSON 同梱 fixture で 1 日 Minute | run-buffer 生成、`C:\tmp\order_flow_06_trades_*.jsonl` 生成 |

**Fixture 準備**:
- `tests/fixtures/strategies/fake_buy_and_hold.py`: 最小 `Strategy` + SCENARIO v1
- `tests/fixtures/run_buffers/estation_reference/`: e-station で過去に生成した正解 run の `meta.json`/`fills.jsonl`/`equity.jsonl` をコピー（schema parity 検証用）
- `tests/fixtures/universe/v05_B_top100_jan1324_minimal.json`: order_flow_06 用に 3 銘柄程度に間引いた universe JSON

## 6. 検証手順（実装完了後の end-to-end）

```powershell
# 1. catalog 整備 (Phase 6 既存手順)
$env:JQUANTS_CATALOG_PATH = "C:\Users\sasai\Documents\The-Trader-Was-Replaced\artifacts\jquants-catalog"
uv run python -m engine build-catalog --granularity MINUTE --start 2025-01-13 --end 2025-01-24

# 2. mean_reversion_01 smoke
uv run python -m engine.strategy_replay run `
  --strategy "C:\Users\sasai\Documents\🐃_blacksheep\strategies\mean_reversion_01.py" `
  --granularity MINUTE
# 出力: <run_id> と run-buffer の絶対 path

# 3. blacksheep ingest (本リポジトリを e-station ロールで指定)
cd C:\Users\sasai\Documents\🐃_blacksheep
uv run python scripts/ingest_run.py <run_id> `
  --e-station C:\Users\sasai\Documents\The-Trader-Was-Replaced

# 4. 確認
# - raw/replay-runs/<run_id>/{meta,fills,equity}.json[l]
# - Silver/runs/<run_id>/summary.json （total_pnl, max_drawdown, trade_count, win_rate, fee_total）
# - wiki/runs/<strategy>-<run_id>.md （AUTO:METRICS / AUTO:LINKS 区間が埋まる）

# 5. order_flow_06 (universe JSON 必要)
uv run python -m engine.strategy_replay run `
  --strategy "C:\Users\sasai\Documents\🐃_blacksheep\strategies\order_flow_06.py" `
  --granularity MINUTE
# C:\tmp\order_flow_06_trades_*.jsonl も生成されることを確認
```

## 7. リスクと注意点

1. **`engine.summary` の名前衝突**: blacksheep が `from engine.summary import compute_summary` を期待する。本リポジトリのモジュール構造で `python/engine/summary.py` を提供し、ingest_run.py の `--e-station` 引数で本リポジトリ root を指せば import 解決される（`run_buffer_path.py` L29-41 が `e_station_root / "python"` を sys.path に追加するため）。**この名前空間契約を壊さないこと**。
2. **Phase 6 既存 catalog 経路の保護**: `python/engine/core.py` / `replay.py` / `reducer.py` / `__main__.py` を **触らない**。Phase 6.5 は新規 sub-package のみで完結。gRPC / Rust side も無変更。
3. **`cache.database=None` 不変条件**: e-station spec.md §3.2 と同様、`BacktestEngineConfig` で persistence を必ず無効化。漏れると `%APPDATA%` 配下に Nautilus cache が湧く。
4. **`commission` の符号**: e-station schema 3.21 で commission は通貨単位・符号保持。`OrderFilled.commissions` のスカラー化で `Money.as_double()` ではなく**符号付き float** にすること。`ingest_run.py` の breakdown が依存。
5. **複数 instrument 時系列マージ**: catalog で複数銘柄を読むと `ts_event` が銘柄間で混在する。`engine.add_data(sorted_bars)` の順序保証が `BacktestEngine` の内部 clock を狂わせないか early test で確認。e-station と同じ stable sort を採用。
6. **strategy_id の trader_id サニタイズ**: `"mean-reversion-01"` → `"REPLAY-MEAN-REVERSION-01"` のような形で Nautilus の `TraderId` 制約 (英大文字/数字/`-`/`_`) を満たすこと。e-station の `_safe_trader_id` をそのまま port。
7. **run-buffer の Windows path 限定**: e-station の `_appdata_run_buffer_root()` は OS 分岐するが、Phase 6.5 は Windows のみで動けばよい（環境が Windows 11）。Linux/macOS 分岐は port するがテストはしない。

## 8. 完了条件 (Definition of Done)

- [ ] `python -m engine.strategy_replay run --help` がエラー無く表示される
- [ ] `mean_reversion_01.py` の smoke test (Minute, 1 銘柄, 1 営業日) が green
- [ ] `order_flow_06.py` の smoke test (Minute, 3 銘柄 universe, 1 営業日) が green
- [ ] 生成された run-buffer を `blacksheep/scripts/ingest_run.py` が無改造で受理し、Silver `summary.json` を生成
- [ ] `test_run_buffer_schema.py` / `test_summary_parity.py` が e-station 既存 run fixture と完全一致
- [ ] Phase 6 経路 (`LoadReplayData` → `ReducerState` → `GetState`) が回帰していない (既存 `tests/python/` 全 green)
- [ ] 本ドキュメントの「検証手順」section の 5 ステップが全て手動再現可能
