# Catalog Fallback Plan (J-Quants CSV → catalog)

## Summary
リプレイ要求で `artifacts/jquants-catalog` に対象 Bar が存在しない（または catalog 未作成）場合、
`DEV_J_QUANTS_CACHE`（`S:/j-quants`）の CSV.gz から `ensure_jquants_catalog` を呼んで
catalog を populate し、通常どおりリプレイする。
granularity は Daily / Minute いずれも対象。
以後の同じ要求は生成済み Bar を直接読む（2 回目は fallback しない）。

## Open Question（仕様決定済み）

`LoadReplayData` の `instrument_ids` と `StartEngine` の SCENARIO instruments は独立。
それぞれが自分の bars を取得できれば成功とする。
両者の不一致チェックは今回スコープ外。

## 変更ファイル（2 ファイル + DataEngine に property 追加）

### 1. `python/engine/core.py` — DataEngine に `jquants_loader_base_dir` property

gRPC 層が `_jquants_loader` の内部構造を直接参照しないよう薄い property を追加。

```python
@property
def jquants_loader_base_dir(self) -> str | None:
    return str(self._jquants_loader.base_dir) if self._jquants_loader else None
```

### 2. `python/engine/core.py` — `load_replay_data` Path1（catalog route）

**初回失敗の catch を `ValueError, FileNotFoundError` 両方に拡大。**
catalog 未作成（`FileNotFoundError`）も populate 対象になる。

```python
try:
    providers[iid] = NautilusBarsReplayProvider(...)
except (ValueError, FileNotFoundError) as first_err:
    if not (self._jquants_loader and start_date and end_date):
        return False, f"{iid}: {first_err}"
    try:
        ensure_jquants_catalog(
            base_dir=self._jquants_loader.base_dir,
            catalog_path=effective_catalog_path,
            instrument_id=iid,
            start_date=start_date,
            end_date=end_date,
            granularity=granularity,
        )
        providers[iid] = NautilusBarsReplayProvider(...)  # retry
    except (ValueError, FileNotFoundError) as e:
        return False, f"{iid}: {e}"
```

- unsupported granularity・IDLE gate は現行挙動を維持。

### 3. `python/engine/server_grpc.py` — StartEngine の呼び出し箇所

**lazy パターン + retry 後に bars が空なら `success=False` で返す。**
warning にして続行すると 0 bars で正常完了に見えるため、明確にエラーにする。

```python
from engine.strategy_runtime.catalog_data_loader import (
    load_bars_for_scenario,
    instruments_from_scenario,
    normalize_granularity,
)
from engine.jquants_to_catalog import ensure_jquants_catalog

bars_by_instrument = load_bars_for_scenario(catalog_path, scenario)

base_dir = self.engine.jquants_loader_base_dir   # property 経由
if base_dir:
    missing = [str(k) for k, v in bars_by_instrument.items() if not v]
    if missing:
        gran = normalize_granularity(scenario["granularity"])
        for symbol in missing:
            try:
                ensure_jquants_catalog(
                    base_dir=base_dir,
                    catalog_path=catalog_path,
                    instrument_id=symbol,
                    start_date=scenario["start"],
                    end_date=scenario["end"],
                    granularity=gran,
                )
            except (ValueError, FileNotFoundError) as e:
                logging.warning("ensure_jquants_catalog skipped %s: %s", symbol, e)
        bars_by_instrument = load_bars_for_scenario(catalog_path, scenario)

# retry 後も空の銘柄があればエラー
still_missing = [str(k) for k, v in bars_by_instrument.items() if not v]
if still_missing:
    return StartEngineResponse(
        success=False,
        error_message=f"No bars after catalog fallback: {still_missing}",
    )
```

- `_jquants_loader.base_dir` の直接参照を `jquants_loader_base_dir` property に変更。
- `instruments_from_scenario` / `normalize_granularity` は `catalog_data_loader` から import 追加。
- `ensure_jquants_catalog` は `jquants_to_catalog` から import 追加。

## ノータッチ

- `catalog_data_loader.py` — 変更なし
- `strategy_replay/cli.py` — 変更なし
- gRPC/proto/Rust UI — public interface 変更なし

## Test Plan

- **Unit / core.py**: Bar 0 件（ValueError）→ fallback 成功（Daily / Minute）。
  catalog 未作成（FileNotFoundError）→ fallback 成功。
  2 回目は `NautilusBarsReplayProvider` 初回で成功し fallback しないことを検証。
- **Unit / server_grpc.py**:
  - bars が空 → `ensure_jquants_catalog` 呼ばれ retry 成功 → `success=True`。
  - bars が既にある → `ensure_jquants_catalog` 呼ばれない → `success=True`。
  - CSV 無し（`ensure_jquants_catalog` 失敗）→ retry 後も bars 空 → `success=False`。
  - **retry 後も bars が空（CSV あるが日付範囲外）→ `success=False`**（silent success を防ぐ）。
- **DataEngine integration**: real catalog に Bar なし・CSV あり の状態で
  `load_replay_data` が成功し、`step_replay()` が正しい OHLC を返すことを検証。
- **Regression**: CSV も無い場合は `core.py` / `server_grpc.py` ともに明確に失敗する。

## Assumptions

- `_jquants_loader` は `--jquants-dir`（`DEV_J_QUANTS_CACHE`）が渡されていれば利用可能。
- `start_date` / `end_date` が空文字の場合は fallback しない。
- `granularity` は `core.py` の入口で既に "Daily" / "Minute" に検証済み。
- `LoadReplayData` と `StartEngine` の instruments 不一致チェックは今回スコープ外。
