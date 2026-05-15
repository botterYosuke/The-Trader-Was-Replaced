# Phase 7.3 - Scenario Sidecar Migration

> **Status**: Draft（実装未着手）
> **Goal**: 戦略 `.py` 内の `SCENARIO` dict を、既存の同名サイドカー `<strategy>.json` に**マージ**する。
> **破壊的変更**: リポ内 8 個の戦略 `.py` から `SCENARIO` / `class Scenario(TypedDict)` を完全削除する。

---

## 1. やること（要旨）

- `python/tests/data/*.py` などに書かれている `SCENARIO: Scenario = {...}` を、同名の `<strategy>.json` の中の `scenario` キーに移動する
- 既存の layout サイドカー [`python/tests/data/test_strategy_daily.json`](../../python/tests/data/test_strategy_daily.json)（中身は `viewport` / `windows` / `strategy_path`）を**そのまま流用**して、`scenario` キーを足す
- `.py` 側からは `SCENARIO` と `class Scenario(TypedDict)` を削除する
- Python CLI（`engine.strategy_replay`）と Rust GUI（`backcast.exe` Run ボタン）の両方で、サイドカー JSON 経由でリプレイが動作することを確認する

---

## 2. サイドカー JSON の構造（最終形）

既存 layout キーの**横**に `scenario` キーを追加する。トップレベル `schema_version` は layout 用としてそのまま残し、SCENARIO 側にも独自の `schema_version`(1/2/3) を入れる（既存 dict そのまま）。

`python/tests/data/test_strategy_daily.json`（移行後）の例：

```json
{
  "schema_version": 1,
  "scenario": {
    "schema_version": 1,
    "instrument": "1301.TSE",
    "start": "2025-01-06",
    "end": "2025-03-31",
    "granularity": "Daily",
    "initial_cash": 1000000
  },
  "viewport": {
    "pan_x": 0.0,
    "pan_y": 0.0,
    "zoom": 1.0
  },
  "windows": [
    { "kind": "RunResult", "visible": true, "position": [402.5, 196.0], "size": [280.0, 160.0], "z": 18.0 },
    { "kind": "Positions", "visible": true, "position": [294.0, -119.0], "size": [280.0, 200.0], "z": 16.0 },
    { "kind": "StrategyEditor", "visible": true, "position": [-72.5, 66.5], "size": [500.0, 400.0], "z": 24.0 }
  ],
  "strategy_path": "C:\\Users\\sasai\\Documents\\The-Trader-Was-Replaced\\python\\tests\\data\\test_strategy_daily.py"
}
```

### キー責務分担

| キー（top-level）  | 持ち主 / 読み手                                        | 内容                                            |
| ------------------ | ------------------------------------------------------ | ----------------------------------------------- |
| `schema_version`   | `src/ui/layout_persistence.rs`（既存）                | layout サイドカー全体のバージョン               |
| `scenario`         | `engine.strategy_runtime.scenario` / `src/ui/scenario_parser.rs`（新規） | SCENARIO の dict（v1/v2/v3）                    |
| `viewport`         | `layout_persistence.rs`（既存）                       | カメラ pan/zoom                                 |
| `windows`          | `layout_persistence.rs`（既存）                       | floating window 配置                            |
| `strategy_path`    | `layout_persistence.rs`（既存）                       | 起動時に自動オープンする `.py` 絶対パス        |

**重要な不変条件**: layout 側読み書きコードは `scenario` キーを**読み飛ばす**（無視する）。SCENARIO 側読み書きコードは layout キー群を**読み飛ばす**。両者は同じ JSON ファイルを共有するが互いに干渉しない。

### SCENARIO のスキーマ（既存と同じ）

| version | 必須キー                                                                              |
| ------- | ------------------------------------------------------------------------------------- |
| 1       | `schema_version`, `instrument` (str), `start`, `end`, `granularity`, `initial_cash`   |
| 2       | `schema_version`, `instruments` (list[str]), `start`, `end`, `granularity`, `initial_cash` |
| 3       | `schema_version`, `instruments` (list[str]) or `instruments_ref` (str), `start`, `end`, `granularity`, `initial_cash` |

`instruments_ref` の `base_dir` はサイドカー JSON の親ディレクトリ（== `.py` の親ディレクトリ）。

---

## 3. 影響範囲（調査結果）

### 3.1 SCENARIO を持つ `.py`（8 ファイル）

すべて `SCENARIO` ブロックと（あれば）`class Scenario(TypedDict)` を**削除**。`LIVE_SCENARIO` はそのまま残す（Phase 8 で扱う）。

| `.py` ファイル                                                                                                        | 現状の SCENARIO 版 | 対応する JSON                                                            |
| --------------------------------------------------------------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------ |
| [`python/tests/data/test_strategy_daily.py`](../../python/tests/data/test_strategy_daily.py) (L37-53)                | v1                 | [`test_strategy_daily.json`](../../python/tests/data/test_strategy_daily.json)（**既存** layout に scenario を追記） |
| [`python/tests/data/test_strategy_minute.py`](../../python/tests/data/test_strategy_minute.py) (L32-48)              | v2                 | `test_strategy_minute.json`（**新規**）                                  |
| [`python/tests/data/test_strategy_trade.py`](../../python/tests/data/test_strategy_trade.py) (L33-49)                | v2                 | `test_strategy_trade.json`（新規）                                       |
| [`python/tests/data/test_strategy_7203_daily.py`](../../python/tests/data/test_strategy_7203_daily.py) (L12-19)      | v1                 | `test_strategy_7203_daily.json`（新規）                                  |
| [`python/tests/data/test_strategy_7203_minute.py`](../../python/tests/data/test_strategy_7203_minute.py) (L12-19)    | v1                 | `test_strategy_7203_minute.json`（新規）                                 |
| [`python/tests/data/pair_trade_minute.py`](../../python/tests/data/pair_trade_minute.py) (L22-39)                    | v2                 | `pair_trade_minute.json`（新規）                                         |
| [`python/tests/fixtures/strategies/fake_market_buy_once.py`](../../python/tests/fixtures/strategies/fake_market_buy_once.py) (L15-22) | v1 | `fake_market_buy_once.json`（新規）                                      |
| [`python/tests/fixtures/strategies/fake_buy_and_hold.py`](../../python/tests/fixtures/strategies/fake_buy_and_hold.py) (L13-20) | v1 | `fake_buy_and_hold.json`（新規）                                         |

### 3.2 SCENARIO を読むコード

| ファイル                                                                                       | 現状                                                              | 変更                                                                                                  |
| ---------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| [`python/engine/strategy_runtime/scenario.py`](../../python/engine/strategy_runtime/scenario.py) | `extract(path)` が `.py` を AST で読む（L43-91）                | `load_scenario(strategy_path)` を新規追加。`<strategy>.json` の `scenario` キーを返す。`extract` は legacy として残置 |
| [`python/engine/strategy_runtime/strategy_loader.py`](../../python/engine/strategy_runtime/strategy_loader.py) (L73) | `extract(path)` を呼ぶ                                          | `load_scenario(path)` に差し替え                                                                     |
| [`src/ui/scenario_parser.rs`](../../src/ui/scenario_parser.rs)                                | 文字列スキャンで `.py` から `SCENARIO` ブロックを抜く（L28-151） | 全削除 → `serde_json` で `<strategy>.json` を読み、`scenario` キーから `ScenarioMetadata` を構築 |
| [`scripts/run_replay.ps1`](../../scripts/run_replay.ps1) (L57)                                | `from engine.strategy_runtime.scenario import extract`            | `load_scenario` に差し替え                                                                            |
| `python/engine/strategy_replay/cli.py`                                                         | loader 経由                                                       | 変更不要                                                                                              |
| `python/engine/server_grpc.py`                                                                 | loader 経由                                                       | 変更不要                                                                                              |
| `python/engine/strategy_runtime/catalog_data_loader.py`                                        | dict を受け取るだけ                                               | 変更不要                                                                                              |
| `python/engine/strategy_runtime/engine_runner.py`                                              | dict を受け取るだけ                                               | 変更不要                                                                                              |

### 3.3 layout サイドカーを読み書きするコード（共存のため確認）

| ファイル                                                                                       | 行                | 現状                                                                                  | 変更                                                                       |
| ---------------------------------------------------------------------------------------------- | ----------------- | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| [`src/ui/layout_persistence.rs`](../../src/ui/layout_persistence.rs) | L160-180, L355-420 | `orig.with_extension("json")` で読み書き                                              | **読む側**: `scenario` キーを保持したまま deserialize / serialize する（破壊しない） |
|                                                                                                | L417, L444-450    | sidecar の存在判定で `path.exists()` を見る                                           | 変更不要                                                                  |
| `src/ui/strategy_editor.rs`                                                                    | -                 | buffer のみ                                                                           | 変更不要                                                                  |
| `src/ui/menu_bar.rs`                                                                           | L271              | `ScenarioMetadata` Resource 経由                                                      | 変更不要                                                                  |

### 3.4 テストコード

| ファイル                                                                                            | 影響                                                                                 |
| --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| [`python/tests/strategy_runtime/test_scenario_extract.py`](../../python/tests/strategy_runtime/test_scenario_extract.py) | `load_scenario` テスト群を新規追加。既存 `extract` テスト群は legacy パスのテストとして温存 |
| [`python/tests/strategy_runtime/test_strategy_loader.py`](../../python/tests/strategy_runtime/test_strategy_loader.py) | synthetic fixture を `.py + .json` 同梱形式に変更                                  |
| `python/tests/strategy_runtime/test_cli.py`                                                         | fixture .py の SCENARIO が消えるので、`fake_market_buy_once.json` も同階層に置く  |
| `python/tests/strategy_runtime/test_catalog_data_loader.py`                                         | 変更なし（dict を直接渡している）                                                 |
| `src/ui/scenario_parser.rs` の `#[cfg(test)] mod tests`                                             | DAILY_SRC / MINUTE_SRC / PAIR_SRC を **JSON 文字列**に書き換え                       |
| `src/ui/layout_persistence.rs` の `#[cfg(test)] mod tests`（`sidecar_layout_round_trip` 等）       | `scenario` キーが round-trip で破壊されないことを assert に追加                    |

### 3.5 ドキュメント

| ファイル                              | 変更                                                                                              |
| ------------------------------------- | ------------------------------------------------------------------------------------------------- |
| [`docs/strategy-replay.md`](../strategy-replay.md) (L62-65) | Sample strategies 節に「SCENARIO は `<strategy>.json` の `scenario` キーに書く」を明記           |

---

## 4. Critical Findings（実装前に必読）

### Sidecar JSON の 3 状態契約（最重要前提）

同じ `<strategy>.json` ファイルが以下の **3 状態すべて**で破綻しないこと。実装はこの契約を満たすこと（F1 / F10 で具体策）:

| 状態           | top-level キー                                  | 例                                          | layout loader   | scenario loader |
| -------------- | ----------------------------------------------- | ------------------------------------------- | --------------- | --------------- |
| layout-only    | `viewport` / `windows` / ...                    | 移行前の `test_strategy_daily.json`         | 正常 apply       | scenario なし → `default()` |
| scenario-only  | `scenario`                                       | 新規 7 ファイル（`test_strategy_minute.json` 等） | no-op（ERROR 出さず） | dict 返す       |
| 両方入り       | `viewport` / `windows` / `scenario`             | T1C 後の `test_strategy_daily.json`         | 正常 apply       | dict 返す       |

「layout loader は scenario キーを保持して書き戻す」「scenario loader は layout キーを読み飛ばす」を全ての save / load 経路で守ること。



### F1: layout サイドカー保存時に SCENARIO を**消さない**ためのマージ必須

`<strategy>.json` は今後「layout データ + SCENARIO データ」を持つ。読み書きする側は**自分のキーだけを触る**ことを徹底する。

#### 問題

[`src/ui/layout_persistence.rs:101-142`](../../src/ui/layout_persistence.rs#L101-L142) の `build_layout` は ECS 状態から `SidecarLayout` を**ゼロから組み立てる**だけで、既存 JSON を読み込んでいない。そのため [`save_layout_to`](../../src/ui/layout_persistence.rs#L144-L148) で serialize すると、`SidecarLayout` 構造体に `scenario` フィールドを足しただけでは **save 時に `None` で上書きされて SCENARIO が消える**。

これは layout の全保存経路（手動 Save / Save As / close 時 autosave / debounced autosave）で起こる。

#### 対応（必須）

1. **`SidecarLayout` 構造体に `scenario: Option<serde_json::Value>` passthrough フィールドを追加**（`#[serde(default, skip_serializing_if = "Option::is_none")]`）

```rust
#[derive(Serialize, Deserialize)]
struct SidecarLayout {
    schema_version: u32,
    viewport: ViewportState,
    windows: Vec<WindowLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    strategy_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_symbol: Option<String>,
    /// SCENARIO 用のキー。layout 側は読まないが、save 時に保持して書き戻す。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scenario: Option<serde_json::Value>,
}
```

2. **`build_layout` を「既存 JSON を読み、scenario を回収して新 layout に merge」する経路に変更**

```rust
fn build_layout(panels: ..., camera: ..., buffer: ...) -> SidecarLayout {
    // ... ECS から viewport / windows / strategy_path を組み立て ...

    // 既存 sidecar に scenario があれば回収して保持する
    let scenario = buffer.original_path.as_ref()
        .map(|p| p.with_extension("json"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("scenario").cloned());

    SidecarLayout { schema_version, viewport, windows, strategy_path, selected_symbol, scenario }
}
```

3. **影響する save 経路すべて**でこの「読んでから merge」を通すこと:
   - `handle_save_layout_system`（L156、メニュー Save）
   - `handle_save_as_layout_system`（L186、Save As — ただし別ファイル名なのでマージしない場合もあり要判断）
   - `handle_close_autosave_system`（L355 周辺、close 時 autosave）
   - debounced autosave 系統（L390-420）

Save As は「明示的に別ファイルへ書く」操作なので、`scenario` を運ぶかどうかは UX 判断。**Save As では scenario をコピーしない**（layout のみ別ファイルへ）方が直感的。手動 Save / autosave だけが `<strategy>.json` を上書きするため、そこに merge を入れる。

### F2: Rust scenario_parser が `buffer.source` ではなくサイドカーを読む

現状 [`src/ui/scenario_parser.rs:4-26`](../../src/ui/scenario_parser.rs#L4-L26) は `StrategyBuffer.source`（== cache 上の `.py` テキスト）を見ている。サイドカー移行後は：

- 発火条件: `buffer.original_path` が変化した瞬間（`buffer.is_changed()` のうち `original_path` だけ）または起動時
- 読み込み元: `<original_path の .py を剥がして .json を付けた path>`
- ファイル不在 / `scenario` キーなし / パース失敗 → `ScenarioMetadata::default()`（Run ボタンがグレーアウト）

`buffer.source`（`.py` 本体）はもう触らない。エディタで `.py` を編集しても SCENARIO は変わらない。

### F3: 外部 blacksheep 戦略の扱い

[`python/tests/strategy_runtime/test_*.py`](../../python/tests/strategy_runtime/) は本リポ外の `C:\Users\sasai\Documents\🐃_blacksheep\strategies\mean_reversion_01.py` などを参照する（skipif で守られている）。これらは `.py` 内に SCENARIO が残ったままなので、移行後は：

- **Python loader**: `<strategy>.json` の `scenario` キーが無ければ、`extract(.py)` にフォールバックし、warn ログを出して既存挙動を維持
- **Rust GUI**: フォールバックなし。サイドカーが無い戦略は GUI から Run できない（仕様として明文化）

これにより外部戦略は CLI からは動き続け、リポ内戦略は GUI/CLI 両方で動く。

### F4: `extract()` API は廃止しない

[`scripts/run_replay.ps1:57`](../../scripts/run_replay.ps1#L57) や外部スクリプトが `extract` を直接 import している可能性。`load_scenario(strategy_path)` を**追加**し、`extract(.py)` は legacy として残す。

### F5: v2 のキー揺れ（`instrument` 単数 vs `instruments` 複数）

[`test_strategy_minute.py:42-48`](../../python/tests/data/test_strategy_minute.py#L42-L48) は v2 だが `instrument` キー（リスト）。[`pair_trade_minute.py:32-39`](../../python/tests/data/pair_trade_minute.py#L32-L39) は v2 で `instruments` キー（リスト）。

[`scenario.py:271-273`](../../python/engine/strategy_runtime/scenario.py#L271-L273) の `validate` 内で正規化が走るので両方通る。**JSON 化時は v2/v3 ともに `instruments`（複数形）に統一**。Python 側 normalize ロジックは legacy `.py` fallback 用に残す。

### F6: ファイル末尾の `LIVE_SCENARIO` は触らない

各 `.py` の `LIVE_SCENARIO` は Phase 8（Live Venue）が使う想定なのでそのまま残す。本 Phase ではコメントアウトすらしない。

### F7: [CRITICAL] GUI Run は cache `.py` を backend に渡すため、元 `<strategy>.json` を読めない

**最重要 finding。これを解決しないと migration 後に GUI Run が必ず壊れる。**

#### 問題

GUI Run の経路：

1. [`src/ui/strategy_editor.rs:221-222`](../../src/ui/strategy_editor.rs#L221-L222) — Run ボタンが `StrategyRunRequested { cache_path }` を発行
2. `src/main.rs:354` — `strategy_file` として **cache path** を gRPC `StartEngine` で送る
3. [`python/engine/server_grpc.py:120`](../../python/engine/server_grpc.py#L120) — backend が `_load_strategy(strategy_file)` を呼ぶ
4. 内部で `load_scenario(cache_path)` が `cache_path.with_name(stem + ".json")` を探す

cache `.py` のパスは `%LOCALAPPDATA%/the-trader-was-replaced/strategy_buffers/<hash>__foo.py`。サイドカー JSON は `<hash>__foo.json` を見にいくが、そんなファイルは存在しない。**元の `foo.json` は別ディレクトリ**。

結果: cache `.py` には SCENARIO がなく、cache JSON も存在しないため `load_scenario` は `ValueError`。GUI Run は migration 直後から動かなくなる。

#### 採用する対応：Cache 時にサイドカー JSON も同梱コピーする

[`src/ui/menu_bar.rs`](../../src/ui/menu_bar.rs) の `open_strategy_buffer_system`（L211 付近、`OpenStrategyRequested` ハンドラ）で cache `.py` を書く処理の直後に、**元 `<strategy>.json` を `<hash>__<strategy>.json` にもコピー**する。

```rust
// cache .py を書いた直後（L228 std::fs::write 成功時の直後）
let original_sidecar = event.path.with_extension("json");
let cache_sidecar = cache_path.with_extension("json");
if original_sidecar.exists() {
    match std::fs::copy(&original_sidecar, &cache_sidecar) {
        Ok(_) => info!("strategy sidecar cached: {:?} -> {:?}", original_sidecar, cache_sidecar),
        Err(err) => warn!("failed to copy sidecar JSON {:?}: {}", original_sidecar, err),
    }
} else {
    // sidecar 不在は外部 blacksheep 戦略のケース。Python loader が .py fallback する
    debug!("no sidecar JSON next to {:?}; cache will rely on .py fallback", event.path);
}
```

#### なぜこの対応か（他案との比較）

| 案 | Pros | Cons |
|---|---|---|
| **A**: cache 時に sidecar をコピー（採用） | RPC schema 不変 / backend 側もコード不変 / 局所的 | sidecar を後から手書き編集してもキャッシュは古いまま（ただし `.py` キャッシュも同じ性質、Open 動作が rebind の役目） |
| B: `StartEngine` RPC に `original_strategy_path` を追加 | キャッシュ不整合なし | proto 変更 / Rust/Python 両側変更 / scope 拡大 |
| C: scenario dict を RPC ペイロードで送る | キャッシュ依存ゼロ / Rust が正源 | Rust が v3 `instruments_ref` 解決をフル実装する必要（F8 参照） / proto 変更 |

#### 不変条件

- Cache .py と cache .json は**ペア**で扱う（同時に書く、同時に消す）
- Open Strategy のたびに cache .json は上書きされる（元 sidecar の最新内容を反映）

### F8: [MAJOR] Rust parser は v3 `instruments_ref` を解決しない（スコープ明示）

Rust 側中間構造は `instruments_ref` を持たないため、v3 で `instruments` キーが欠落し `instruments_ref` のみの sidecar は GUI Run できない（`ScenarioMetadata.instruments.is_empty()` で Run ブロック）。

**選択肢**:

- (i) GUI を v3 `instruments_ref` 非対応とする（**採用**） — `docs/strategy-replay.md` と AC に明示
- (ii) Rust 側でも JSON Pointer 解決を実装 — scope 拡大、universe.json の読み込みパス解決が必要

採用は (i)。理由: 現状リポ内 8 戦略に v3 はない。v3 を使う外部 blacksheep 戦略は CLI から動かす。GUI 対応が必要になったら別 Phase で扱う。

なお `instruments` キーがすでに resolve 済みで JSON 内に書かれているケース（事前解決済みサイドカー）は GUI 対応。

### F9: [MAJOR] Python 側 `validate()` は normalize を**返さない** → `load_scenario` で明示 normalize 必須

[`scenario.py:271-273`](../../python/engine/strategy_runtime/scenario.py#L271-L273) は v2/v3 で `instrument`（単数）→ `instruments`（複数）の正規化を**ローカル変数で行うだけ**で、引数 dict も戻り値も変更しない。

```python
if sv in (2, 3) and "instrument" in d and "instruments" not in d:
    d = dict(d)
    d["instruments"] = d.pop("instrument")  # ← この d はローカルのみ
```

結果: legacy `.py` fallback ルートで v2 戦略を読むと、上位の `load_scenario` が返す dict はキー揺れのまま。下流 [`catalog_data_loader.py:27-31`](../../python/engine/strategy_runtime/catalog_data_loader.py#L27-L31) は `"instrument" in scenario` で分岐するため、v2 で `instrument: ["A", "B"]` だった場合 `[["A", "B"]]`（list-of-list）を返す事故が起きうる。

#### 対応

`normalize_scenario(d) -> dict` を `scenario.py` で public 化し、`load_scenario()` 内で `resolve_refs → normalize_scenario → validate` の順で必ず通す。

```python
def normalize_scenario(d: dict) -> dict:
    """v2/v3 の "instrument" キー → "instruments" 正規化を実施した新 dict を返す。"""
    sv = d.get("schema_version")
    if sv in (2, 3) and "instrument" in d and "instruments" not in d:
        out = dict(d)
        out["instruments"] = out.pop("instrument")
        return out
    return d
```

`validate()` 内の重複 normalize ロジックは削除し、`validate()` は「正規化済み dict が来る」前提に変更。**CLI 側の `--start` / `--end` / `--granularity` 上書き経路（[`cli.py:130-140`](../../python/engine/strategy_replay/cli.py#L130-L140)）も `normalize_scenario` を通す**。

### F10: [CRITICAL] scenario-only `<strategy>.json` を layout loader が誤って読みにいく

#### 問題

[`src/ui/layout_persistence.rs:444-450`](../../src/ui/layout_persistence.rs#L444-L450) の `watch_open_strategy_for_sidecar_system` は、Open された `.py` の隣に同名 `.json` があれば**無条件に layout sidecar として load** する。

T2 で新規作成する 7 個の JSON はトップレベル `scenario` キーのみで、`schema_version` / `viewport` / `windows` を持たない。現在の [`SidecarLayout` 構造体](../../src/ui/layout_persistence.rs#L15) はこれらを必須フィールドにしているため、`serde_json::from_str::<SidecarLayout>` が失敗し、Open のたびに ERROR ログが出る（cache copy や scenario_parser には影響しないが、UX が壊れる）。

#### 対応（必須）

`SidecarLayout` のすべての layout フィールドを **optional / default 可能**にし、layout 適用側を「不在なら no-op」にする。

```rust
#[derive(Serialize, Deserialize, Default)]
struct SidecarLayout {
    #[serde(default)]
    schema_version: Option<u32>,
    #[serde(default)]
    viewport: Option<ViewportState>,
    #[serde(default)]
    windows: Vec<WindowLayout>,         // 空がデフォルト
    #[serde(default, skip_serializing_if = "Option::is_none")]
    strategy_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scenario: Option<serde_json::Value>,
}
```

挙動:

- **scenario-only JSON**: `viewport=None` / `windows=[]` で deserialize 成功。`apply_layout` は viewport を変えず windows を spawn しない（no-op）。`scenario` キーだけ保持される
- **layout-only JSON**（既存 `test_strategy_daily.json` の現状）: `scenario=None`、layout 部分は正常 apply
- **両方入り**（T1C 後の `test_strategy_daily.json`）: 両方正常

`apply_layout_system` 内のロジック分岐を `viewport: None → camera 触らない` / `windows.is_empty() → 何も spawn しない` に修正。`schema_version: None` のときは「未保存 layout」として扱い ERROR を出さない。

これにより 1 ファイルで「scenario のみ」「layout のみ」「両方」の 3 状態すべてが破綻なく扱える。

### F11: [CRITICAL] cache `.py` 実行時の v3 `instruments_ref` base_dir 問題

#### 問題

[`scenario.py:130-189`](../../python/engine/strategy_runtime/scenario.py#L130-L189) の `resolve_refs(d, base_dir=...)` は v3 の `instruments_ref` を `base_dir` 起点で解決する。[`strategy_loader.py:76`](../../python/engine/strategy_runtime/strategy_loader.py#L76) は `base_dir=path.parent` を渡す。

GUI Run 経路では `path` は cache `.py`（`%LOCALAPPDATA%/.../strategy_buffers/<hash>__foo.py`）。`base_dir` が cache dir になるため、`instruments_ref: "universe.json#/instruments"` のような相対参照は cache dir に存在しない `universe.json` を探して失敗する。

#### 対応

F8（Rust GUI が v3 `instruments_ref` 非対応）と整合させるため、**GUI Run は v3 `instruments_ref` を一律ブロック**する。具体策:

- Rust 側 `scenario_parser.rs` は `instruments_ref` を持ち `instruments` 空の sidecar に対して `ScenarioMetadata.instruments` を空のまま返す（既に F8 で予定）
- Run ボタンは `instruments.is_empty()` で blocked（既存挙動）
- Python 側 `load_scenario` が v3 `instruments_ref` を解決する経路は CLI 専用とし、ドキュメント明示

CLI（`run_replay.ps1` 経由）で動かす場合は `path = 元 .py`（cache を使わない）なので、`base_dir = 元ディレクトリ`となり `universe.json` も解決できる。

代替案（採用しない）: cache 時に `instruments_ref` の解決済み `instruments` を cache JSON に注入する / 参照ファイルも cache にコピーする。スコープ拡大のため Phase 7.3 では採用しない。

### F12: [MAJOR] Rust/Python の scenario 二重読み込みの一致を保つ

GUI Run 経路では「Rust が sidecar を読んで `ScenarioMetadata` を作る」+「backend が sidecar を再読み込みする」の二段構えになる。両者が同じ内容を読まないと `LoadReplayData` 成功 → `StartEngine` 失敗のようなずれが起きる。

F7 の cache copy 対応で「両者とも cache 側 sidecar を読む」ことになるので、内容は自然に一致する（Rust は元 sidecar を読み、backend は cache 後の sidecar を読むが、cache は Open 時の元 sidecar をそのままコピーしているので同一）。

ただし以下のレースには注意:

- Open Strategy 後、Run 押下前に元 sidecar をエディタ外で書き換えた場合 → Rust の `ScenarioMetadata` は古い元 sidecar、backend は cache（= Open 時の元 sidecar）を読む → 古い同士で一致するが、ユーザー期待とずれる

このレース挙動はドキュメントで明示（`.py` cache の挙動と同じ）。

---

## 5. タスクと依存関係

```
T1A: Python loader     T1B: Rust parser    T1C: test_strategy_daily.json    T1D: Layout merge
  scenario.py 改修       scenario_parser.rs    scenario キー追記              build_layout が
  normalize_scenario     v1/v2 deserialize                                   既存 JSON から
  load_scenario                                                              scenario 回収
        │                       │                   │                          │
        └─────┬─────────────────┴───────┬───────────┘                          │
              │                         │                                       │
              │                  T1E: Cache sidecar copy                       │
              │                  open_strategy_buffer_system で                │
              │                  <hash>__foo.json も同梱コピー                 │
              │                         │                                       │
              └─────────────┬───────────┴───────────────────────────────────────┘
                            ↓
T2: 残り 7 ファイル移行（.py から SCENARIO 削除 + .json 新規 or 追記）
                            ↓
T3: テスト更新（pytest + cargo test）
                            ↓
T4: docs / scripts / コメント更新
                            ↓
T5: E2E 検証（CLI / GUI Run / blacksheep 戦略 fallback / layout 上書き不変性）
```

依存のない T1A / T1B / T1C / T1D / T1E は `/parallel-agent-dev` で並列起動。ただし **T1E は T1A / T1B の API が固まらないと書きづらい**ので、T1A/T1B の inferface 凍結後に開始する選択肢もある。

---

## 6. タスク詳細

### T1A — Python loader（`scenario.py` / `strategy_loader.py`）

**Files**:
- `python/engine/strategy_runtime/scenario.py`（編集）
- `python/engine/strategy_runtime/strategy_loader.py`（L73 のみ）
- `python/engine/strategy_replay/cli.py`（L130-140 のオーバーライド後にも normalize を通す）

**実装**:

```python
def _sidecar_path(strategy_path: Path) -> Path:
    """foo.py → foo.json（同一ディレクトリ）"""
    return strategy_path.with_name(strategy_path.stem + ".json")


def normalize_scenario(d: dict) -> dict:
    """v2/v3 の "instrument" キー → "instruments" 正規化を実施した新 dict を返す。
    既に正規化済みの dict はそのまま返す（idempotent）。
    """
    sv = d.get("schema_version")
    if sv in (2, 3) and "instrument" in d and "instruments" not in d:
        out = dict(d)
        out["instruments"] = out.pop("instrument")
        return out
    return d


def load_scenario(strategy_path: Path) -> dict:
    """サイドカー <strategy>.json の "scenario" キーを返す。
    必ず resolve_refs → normalize_scenario → validate の順で通す。

    フォールバック順:
      1. <strategy>.json が存在し scenario キーがある → JSON ロード
      2. <strategy>.py 内に SCENARIO がある → extract() に委譲 + WARN ログ
      3. どちらも無ければ ValueError
    """
    sidecar = _sidecar_path(strategy_path)
    if sidecar.exists():
        try:
            doc = json.loads(sidecar.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise ScenarioValidationError(f"invalid JSON in {sidecar}: {exc}") from exc
        if isinstance(doc, dict) and "scenario" in doc:
            d = doc["scenario"]
            d = resolve_refs(d, base_dir=sidecar.parent)
            d = normalize_scenario(d)
            validate(d)
            return d
        # サイドカーはあるが scenario キーが無い → .py にフォールバック

    if strategy_path.exists():
        d = extract(strategy_path)
        if d is not None:
            log.warning(
                "SCENARIO loaded from .py (legacy); migrate to %s",
                sidecar.name,
            )
            d = resolve_refs(d, base_dir=strategy_path.parent)
            d = normalize_scenario(d)
            validate(d)
            return d

    raise ValueError(
        f"SCENARIO not found: looked for 'scenario' key in {sidecar} "
        f"and SCENARIO in {strategy_path}"
    )
```

さらに `validate()` 内の重複正規化ロジック（L271-273）は削除し、「正規化済み dict が来る」前提に変更。

CLI 側（`engine.strategy_replay.cli._cmd_run`）も override 後に `normalize_scenario` を通す:

```python
# cli.py L130 周辺の override 後に追加
from engine.strategy_runtime.scenario import normalize_scenario
scenario = normalize_scenario(scenario)
```

**TDD**:

1. RED: `test_load_scenario_prefers_sidecar` — `.json` の `scenario` キーが勝つ
2. RED: `test_load_scenario_falls_back_to_py_with_warning` — サイドカーに `scenario` 無 → `.py` から読みつつ WARN
3. RED: `test_load_scenario_raises_when_both_absent`
4. RED: `test_load_scenario_invalid_json_raises`
5. RED: `test_load_scenario_layout_only_json_falls_back_to_py`（既存 layout サイドカーで scenario キーが無いケース）
6. RED: `test_load_scenario_normalizes_v2_instrument_key` — legacy `.py` 経由で `{"schema_version": 2, "instrument": ["A","B"]}` → 戻り値 dict が `instruments` キーで `["A","B"]`
7. RED: `test_normalize_scenario_idempotent` — 既に正規化済みの dict をもう一度通しても変わらない
8. RED: `test_load_scenario_with_complex_suffix` — `foo.bar.py` → `foo.bar.json` を見にいく
9. GREEN: 実装
10. REFACTOR: `_sidecar_path` を public で公開

**AC**:
- `uv run pytest python/tests/strategy_runtime/test_scenario_extract.py -v` 全緑
- `uv run pytest python/tests/strategy_runtime/test_strategy_loader.py -v` 全緑
- `catalog_data_loader.instruments_from_scenario(load_scenario(p))` が常に `list[str]` を返す（list-of-list 事故が起きない）

### T1B — Rust parser（`scenario_parser.rs`）

**Files**: [`src/ui/scenario_parser.rs`](../../src/ui/scenario_parser.rs)

**スコープ（F8 参照）**: v3 で `instruments_ref` のみ持つ sidecar は **GUI 非対応**。`instruments` がすでに存在するケースのみサポート。

**実装方針**:

- 文字列スキャン関数群（`extract_scenario_block` / `parse_string_field` / `parse_int_field` / `parse_string_or_list_field`）を**全削除**
- `serde` で `scenario` キーをデシリアライズする中間構造を定義
- 発火条件を `buffer.is_changed()` → `buffer.original_path` 変化検知（`Local<Option<PathBuf>>` でキャッシュ）に変更
- 読み込み元は `original_path.with_name(stem + ".json")`
- `instruments_ref` フィールドは deserialize するが解決しない。`instruments` が空のままなら `ScenarioMetadata.instruments.is_empty()` → Run ブロック（既存挙動）

中間構造：

```rust
#[derive(serde::Deserialize)]
struct SidecarRoot {
    #[serde(default)]
    scenario: Option<ScenarioFile>,
}

#[derive(serde::Deserialize)]
struct ScenarioFile {
    schema_version: Option<u32>,
    // v1: str / v2,v3: list の両対応
    #[serde(default)]
    instrument: Option<StringOrList>,
    #[serde(default)]
    instruments: Option<Vec<String>>,
    /// v3 で universe.json などを参照する場合のキー。Rust では解決しない（F8）。
    /// deserialize はするが ScenarioMetadata には載せない。
    #[serde(default)]
    instruments_ref: Option<String>,
    start: Option<String>,
    end: Option<String>,
    granularity: Option<String>,
    initial_cash: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StringOrList { One(String), Many(Vec<String>) }
```

`ScenarioFile` → `ScenarioMetadata` 変換ロジックで「`instruments` 優先、無ければ `instrument` を 1 要素 list 化」する（既存 fallback 順）。`instruments_ref` のみの場合は `instruments` が空のまま返り、Run ボタンがブロックされる（仕様）。

**TDD**:

1. RED: `test_parse_v1_from_json` — `instrument: "1301.TSE"` が `instruments: vec!["1301.TSE"]` に
2. RED: `test_parse_v2_from_json` — `instruments: [...]` をそのまま
3. RED: `test_parse_pair_multi` — `instruments: ["A","B"]`
4. RED: `test_missing_sidecar_returns_default` — ファイル不在
5. RED: `test_sidecar_without_scenario_key_returns_default` — layout のみの旧 JSON
6. RED: `test_malformed_json_returns_default_and_warns`
7. RED: `test_v3_instruments_ref_only_returns_empty_instruments` — `instruments_ref` のみで `instruments` 不在 → `instruments.is_empty()`、Run ブロック想定
8. RED: `test_v3_resolved_instruments_works` — 事前解決済みの `instruments` リスト付き v3 は通る
9. GREEN: 実装
10. REFACTOR: 旧 string-scan 関数を削除

**AC**:
- `cargo test -p backcast --lib ui::scenario_parser` 全緑
- `cargo clippy -- -D warnings` クリーン

### T1C — `test_strategy_daily.json` の修正（テストデータ）

**Files**:
- `python/tests/data/test_strategy_daily.json`（**既存ファイルを修正**）

**作業**:

既存ファイルの top-level に `scenario` キーを追加：

```json
{
  "schema_version": 1,
  "scenario": {
    "schema_version": 1,
    "instrument": "1301.TSE",
    "start": "2025-01-06",
    "end": "2025-03-31",
    "granularity": "Daily",
    "initial_cash": 1000000
  },
  "viewport": { ... 既存 ... },
  "windows": [ ... 既存 ... ],
  "strategy_path": "..."
}
```

**AC**:
- T1A の `load_scenario(test_strategy_daily.py)` がこのファイルから dict を返す
- T1B の `parse_scenario_system` がこのファイルから `ScenarioMetadata` を構築する
- 既存 layout 読み込みコードが round-trip でこの `scenario` キーを破壊しない（T1D 完了後）

### T1D — Layout tolerant load + scenario passthrough（F1 / F10 対応）

**Files**: [`src/ui/layout_persistence.rs`](../../src/ui/layout_persistence.rs)

**契約**: 同じ `<strategy>.json` が「layout-only」「scenario-only」「両方入り」の 3 状態すべてで壊れず動くこと。

**作業**:

1. **`SidecarLayout` の layout フィールドをすべて optional / default 可能にする**（F10）
   - `schema_version`, `viewport` を `Option<>` 化
   - `windows` は `Vec<_>` のままだが `#[serde(default)]` で空配列を許容
   - `scenario: Option<serde_json::Value>` を passthrough として追加（F1）
2. **`apply_layout_system` を「不在キーは no-op」に修正**
   - `viewport.is_none()` → カメラを触らない
   - `windows.is_empty()` → window を spawn しない
   - `schema_version.is_none()` → ERROR を出さず WARN もしくは debug ログに留める
3. **`build_layout` を改修**し、既存 sidecar JSON があれば `scenario` キーを読み取って新 layout に含める
4. 影響する save 経路すべてが新 `build_layout` を通ることを確認:
   - `handle_save_layout_system`
   - `handle_close_autosave_system`
   - debounced autosave 系統
   - **`handle_save_as_layout_system` は除外**（明示的に別ファイルへ書く操作のため scenario を運ばない）

**TDD**:

1. RED: `test_deserialize_scenario_only_sidecar` — `{"scenario": {...}}` だけの JSON が `from_str::<SidecarLayout>` で成功し、layout 部分は all None / empty
2. RED: `test_deserialize_layout_only_sidecar` — 既存形式（scenario なし）が今まで通り読める
3. RED: `test_deserialize_combined_sidecar` — 両方入りが正しく分離して読める
4. RED: `test_apply_layout_scenario_only_is_noop` — scenario-only を apply してもカメラ・windows が変わらない
5. RED: `test_build_layout_recovers_scenario_from_existing_sidecar` — `scenario` 付き JSON が存在する状態で `build_layout` を呼ぶと `SidecarLayout.scenario` が `Some` で返る
6. RED: `test_save_layout_preserves_scenario_key` — `scenario` 付き JSON を保存後に再読み込みすると `scenario` が semantic に残る
7. RED: `test_save_as_does_not_carry_scenario` — Save As は scenario を運ばない
8. RED: `test_autosave_preserves_scenario_through_window_drag` — window 移動 → autosave → `scenario` が消えていない
9. GREEN: 実装
10. REFACTOR: 既存 `sidecar_layout_round_trip` テストを `scenario` 含む形に拡張

**AC**:
- `cargo test -p backcast --lib ui::layout_persistence` 全緑
- 7 個の scenario-only JSON を Open しても layout 側で ERROR ログが出ない
- `test_strategy_daily.json` を window 1 つ動かして autosave した後、PowerShell で `(Get-Content path.json | ConvertFrom-Json).scenario` が非 null

### T1E — Cache sidecar copy（F7 対応・最重要）

**Files**:
- [`src/ui/menu_bar.rs`](../../src/ui/menu_bar.rs) の `open_strategy_buffer_system`（L211 周辺）

**作業**:

cache `.py` を書き込む処理の直後に、元 `<strategy>.json` を `<hash>__<strategy>.json` にコピーする。元 sidecar が存在しなければ何もしない（外部 blacksheep 戦略は Python loader の `.py` fallback で動く）。

```rust
// cache .py 書き込み成功後（既存 std::fs::write(&cache_path, &source) の直後）
let original_sidecar = event.path.with_extension("json");
let cache_sidecar = cache_path.with_extension("json");
if original_sidecar.exists() {
    match std::fs::copy(&original_sidecar, &cache_sidecar) {
        Ok(_) => info!(
            "strategy sidecar cached: {:?} -> {:?}",
            original_sidecar, cache_sidecar
        ),
        Err(err) => warn!(
            "failed to copy sidecar JSON {:?}: {}",
            original_sidecar, err
        ),
    }
}
```

**TDD**:

1. RED（integration）: `test_open_strategy_copies_sidecar` — tmp に `foo.py` + `foo.json` を作り、`OpenStrategyRequested` を発行後 `<cache>/<hash>__foo.json` が存在し内容が一致
2. RED: `test_open_strategy_without_sidecar_no_copy_no_error` — sidecar 不在でもエラーが出ず、cache .py は書ける
3. RED: `test_reopen_strategy_overwrites_cache_sidecar` — 元 sidecar を編集して再 Open すると cache sidecar も更新
4. GREEN: 実装

**AC**:
- 上記テスト全緑
- GUI で `test_strategy_daily.py` を Open すると `%LOCALAPPDATA%/the-trader-was-replaced/strategy_buffers/` に `<hash>__test_strategy_daily.py` と `<hash>__test_strategy_daily.json` が両方できる

### T2 — 残り 7 ファイル移行

**Files**:

新規 `.json`（7 個）:
- `python/tests/data/test_strategy_minute.json`
- `python/tests/data/test_strategy_trade.json`
- `python/tests/data/test_strategy_7203_daily.json`
- `python/tests/data/test_strategy_7203_minute.json`
- `python/tests/data/pair_trade_minute.json`
- `python/tests/fixtures/strategies/fake_market_buy_once.json`
- `python/tests/fixtures/strategies/fake_buy_and_hold.json`

これら 7 個はトップレベルに **`scenario` キーだけ**を持つ（layout を兼ねないファイル）。例：

```json
{
  "scenario": {
    "schema_version": 2,
    "instruments": ["1301.TSE", "7203.TSE"],
    "start": "2025-01-06",
    "end": "2025-01-10",
    "granularity": "Minute",
    "initial_cash": 1000000
  }
}
```

**`.py` 削除・更新作業（各ファイル）**:

1. `from typing import TypedDict` 行（他で使っていなければ）削除
2. `class Scenario(TypedDict): ...` ブロック削除
3. `SCENARIO: Scenario = {...}` または `SCENARIO: dict = {...}` ブロック削除
4. `LIVE_SCENARIO` は残す（Phase 8 で扱う）
5. docstring・コメント内の "SCENARIO" 言及を sidecar 前提に書き直す:
   - 「`SCENARIO` で指定する」「`SCENARIO` 定数」「SCENARIO はここに書く」系の説明文 → 「`<strategy>.json` の `scenario` キーで指定する」に
   - 起動コマンド例（`--instrument 1301.TSE`）はそのままで OK（CLI 引数として残る）
6. ファイル末尾の `from nautilus_trader.model.data import ...` 等の import が `SCENARIO` ブロックの後ろに置かれているケース（例: `test_strategy_daily.py:66-70`）は **上部に移動**して PEP8 violation を解消する

**v2 のキー正規化**: `.json` 側は `instruments`（複数形・リスト）に統一する。`.py` 内に `instrument: ["1301.TSE"]`（v2 で単数キー）だったものも `instruments: ["1301.TSE"]` に直す。

**AC**:
- `grep -RE "^SCENARIO\s*[:=]|class Scenario\(TypedDict\)" python/tests/data/ python/tests/fixtures/` で 0 件
- すべての `.py` に対応する `.json` が存在し、`load_scenario(.py)` で dict が取れる
- pytest で legacy fallback の WARN ログが出ない（リポ内戦略は全て JSON ルートを通る）

### T3 — テスト更新

並列で実施可：

**Python — 直接影響を受けるテストファイル**:

- `python/tests/strategy_runtime/test_scenario_extract.py`: `load_scenario` テスト群を追加（synthetic fixture: `tmp_path` に `.py` と `.json` の両方を書く）。F9 の normalize テスト群を含む
- `python/tests/strategy_runtime/test_strategy_loader.py`: `_write_strategy` を sidecar 形式に寄せる。legacy SCENARIO in .py のテストは別関数で残す
- `python/tests/strategy_runtime/test_cli.py`: fixture `fake_market_buy_once.json` が同梱されていれば動く（追加ファイルの存在確認のみ）

**Python — gRPC / E2E 経路で影響を受けるテストファイル**（指定）:

- `python/tests/test_grpc_control.py`: `test_strategy_7203_daily.py` / `test_strategy_7203_minute.py` を直接 `StartEngine` に渡している。これらの `.py` から SCENARIO を消したあと、sidecar 経由で StartEngine が成功すること（StartEngine response success / current_state 遷移）
- `python/tests/test_fake_strategy_e2e.py`: `fake_market_buy_once.py` / `fake_buy_and_hold.py` を使う。sidecar 経由でフル E2E が動くこと
- `python/tests/test_blacksheep_ingest_compat.py`: blacksheep 戦略を読む経路。legacy `.py` fallback が WARN ログ付きで動くこと
- `python/tests/test_order_flow_06_smoke.py`: v3 `instruments_ref` を使う。CLI 経路で動き続けること（GUI 非対応の確認は別途）
- `python/tests/strategy_runtime/test_engine_runner.py`: `fake_buy_and_hold` fixture を使う。sidecar 経由でも動くこと

これらは「既存テストが壊れないこと」を確認するだけで、原則テスト本体を変更しない（fixture 側 `.py` から SCENARIO を消すことで自動的に sidecar 経由になる）。

**Rust**:
- `src/ui/layout_persistence.rs` の `sidecar_layout_round_trip` テストに「`scenario` キーが round-trip で生き残る」+ 「scenario-only JSON が deserialize に成功する」を追加（T1D で実施）

**新規 critical テスト**:
- Python: cache 経路再現 — cache `.py` だけ存在し cache `.json` が無いとき、`load_scenario(cache_path)` が `.py` も SCENARIO なしのため `ValueError` で落ちることをテスト化（T1E の cache copy が抜けると StartEngine が失敗することの検知になる）

**AC**:
- `uv run pytest python/tests/ -v` 全緑
- `cargo test --workspace` 全緑

### T4 — docs / scripts / コメント更新

**Files**:
- [`docs/strategy-replay.md`](../strategy-replay.md) (L53 周辺と Sample strategies 節): 「SCENARIO は `<strategy>.json` の `scenario` キーに書く」を明記。`.py` 内 `SCENARIO` 前提の説明箇所を全削除し、サイドカー前提に書き直す。`.py` 内に SCENARIO がある外部戦略は **Python CLI からのみ** legacy fallback で動くことも記載。GUI は v3 `instruments_ref` 非対応であることも明示
- [`scripts/run_replay.ps1`](../../scripts/run_replay.ps1) (L57): `extract` → `load_scenario` に置換
- [`python/engine/strategy_replay/cli.py:34`](../../python/engine/strategy_replay/cli.py#L34): help 文言 `"must contain SCENARIO and a Strategy subclass"` を `"must contain a Strategy subclass; SCENARIO is loaded from the sidecar <strategy>.json (legacy: SCENARIO in .py)"` に修正
- [`src/trading.rs:151`](../../src/trading.rs#L151): `Scenario fields extracted from SCENARIO dict in the strategy .py file` というコメントを「the strategy's `<strategy>.json` sidecar」に書き直し

**AC**:
- `.\scripts\run_replay.ps1 -Strategy python\tests\data\test_strategy_daily.py` exit 0
- `uv run python -m engine.strategy_replay run --help` で新 help 文言が表示される

### T5 — E2E 検証

**Steps**:

1. CLI: `uv run python -m engine.strategy_replay run --strategy python/tests/data/test_strategy_daily.py --catalog artifacts/jquants-catalog --run-buffer-dir tmp/rb` exit 0、`equity_points > 0`
2. CLI: 同じく `pair_trade_minute.py` で exit 0（v2 `instruments` 経路の確認）
3. CLI: `--start 2025-02-01 --end 2025-02-28` 上書きで normalize 後の dict が正しく流れる（v2 戦略で `instruments` キーになっていることを log で確認）
4. **GUI Run 経路**: backend + GUI 起動 → `Open Strategy` で `test_strategy_daily.py` → cache ディレクトリに `<hash>__test_strategy_daily.py` と `<hash>__test_strategy_daily.json` が両方できることを確認 → Strategy Editor の Run ボタン押下 → state RUNNING → IDLE、Run Result Panel に summary 表示
5. **layout merge 不変性**: 戦略を Open した状態で window を動かして autosave 発火 → `test_strategy_daily.json` を再 read → `scenario` キーが（semantic に）残っていること
   - PowerShell: `(Get-Content python\tests\data\test_strategy_daily.json | ConvertFrom-Json).scenario` が非 null
6. **GUI Run の cache 不整合検知**: cache ディレクトリの `<hash>__test_strategy_daily.json` だけを手動削除して GUI Run → `STRATEGY_LOAD_ERROR` が backend ログに出る（StartEngine が落ちることを「正しく」検知できる）
7. external blacksheep 戦略を CLI で動かす → WARN ログ「SCENARIO loaded from .py (legacy)」が出て exit 0
8. **v3 instruments_ref GUI 非対応の確認**: `instruments_ref` のみの sidecar を作って GUI で開く → Run ボタンがグレーアウト（`ScenarioMetadata.instruments` 空でブロック）
9. **scenario-only JSON 3 状態確認**: 新規 7 個の scenario-only JSON のうち 1 つ（例: `test_strategy_minute.py`）を Open → layout loader が ERROR を出さず、scenario_parser が正しく `ScenarioMetadata` を構築すること

**AC**:
- 全 step が期待通り
- リポ内戦略でリプレイ実行時に legacy fallback の WARN ログが出ない（リポ内は全て sidecar 経路を通る）
- Step 5 と Step 6 の挙動が docs に明記されている

---

## 7. Acceptance Criteria（フェーズ全体）

- [ ] `uv run pytest python/tests/ -v` 全緑
- [ ] `cargo test --workspace` 全緑
- [ ] `cargo clippy -- -D warnings` クリーン
- [ ] **scope 限定 残骸チェック AC**（PowerShell / rg ベース）:
  - `rg -n "^SCENARIO\s*[:=]|class Scenario\(TypedDict\)" python/tests/data python/tests/fixtures/strategies` → 0 件
  - `rg -n "\bSCENARIO\b|TypedDict" python/tests/data python/tests/fixtures/strategies` の結果が **`LIVE_SCENARIO` を含む行のみ**であること（コメント / docstring / 未使用 import がないこと）
  - `rg -n "from typing import TypedDict" python/tests/data python/tests/fixtures/strategies` → 0 件（移行で全削除）
- [ ] 8 個の `.py` それぞれに対応する `<strategy>.json`（`scenario` キーあり）が存在する
- [ ] `test_strategy_daily.json` には layout キー（viewport / windows / strategy_path）と `scenario` キーが**共存**している
- [ ] layout 自動保存後も `scenario` キーが破壊されない（T1D の round-trip テスト + T5 step 5）
- [ ] **GUI Open Strategy 時に cache sidecar JSON が同梱コピーされる**（T1E）
- [ ] **`<strategy>.json` 単独で読んだ scenario と、cache `<hash>__<strategy>.json` で読んだ scenario が一致**（F10）
- [ ] `load_scenario` の戻り値が v2/v3 で必ず `instruments` キーを持つ（F9 normalize）
- [ ] `.\scripts\run_replay.ps1 -Strategy python\tests\data\test_strategy_daily.py` exit 0
- [ ] GUI で `test_strategy_daily.py` を Open → Run が完走
- [ ] `docs/strategy-replay.md` が更新済み
- [ ] `python/engine/strategy_replay/cli.py` の `--help` 文言と `src/trading.rs:151` コメントが更新済み

---

## 8. Risk & Mitigation

| Risk                                                                                       | Likelihood | Impact | Mitigation                                                                                   |
| ------------------------------------------------------------------------------------------ | ---------- | ------ | -------------------------------------------------------------------------------------------- |
| [CRITICAL] **GUI Run が cache `.py` 経路で sidecar を見つけられない（F7）**                       | **High**   | **High** | T1E で `open_strategy_buffer_system` に sidecar 同梱コピーを実装。T3/T5 で cache 経路テスト |
| [CRITICAL] layout save 時に `scenario` キーが**消える**（F1）                                     | **High**   | **High** | T1D で `build_layout` が既存 sidecar から `scenario` を回収して merge。round-trip テスト必須 |
| [CRITICAL] scenario-only JSON を layout loader が deserialize に失敗（F10）                       | **High**   | **High** | T1D で `SidecarLayout` の layout フィールドを optional / default 化。apply_layout を no-op tolerant に |
| [CRITICAL] v3 `instruments_ref` の base_dir が cache dir になり参照ファイル見失う（F11）          | Mid        | High    | GUI Run は `instruments_ref` を一律ブロック（Run ボタングレーアウト）。CLI 経路は元 .py の親で解決 |
| [MAJOR] v2/v3 normalize が `load_scenario` 戻り値に反映されない（F9）                          | Mid        | Mid    | T1A で `normalize_scenario` を public 化し `load_scenario` で明示的に通す。CLI override 経路も含む |
| Rust GUI で v3 `instruments_ref` を解決できない（F8）                                     | Low        | Low    | スコープ外と明示。docs と AC に記載。Run ブロックで安全側に倒れる                            |
| Rust と Python が異なる sidecar を読んで状態不整合（F10）                                  | Low        | Mid    | T1E の cache copy で「Rust = 元 sidecar, Python = cache sidecar = 元 sidecar のコピー」と保証 |
| 外部 blacksheep 戦略が legacy fallback ルートで壊れる                                       | Low        | Mid    | T1A で fallback パスをテスト化（synthetic fixture）                                          |
| `Path.with_name(stem + ".json")` が複合拡張子（`foo.bar.py`）で意図しない動作              | Low        | Low    | T1A で `foo.bar.py` → `foo.bar.json` を期待するテストを追加                                  |
| Rust GUI で `<strategy>.json` 不在 → Run ボタン永遠グレーアウト                            | Mid        | Low    | docs に「外部戦略を GUI で動かすには `.json` 作成が必須」と明記                              |
| v2 で `instrument` と `instruments` のキー揺れが残る                                       | Mid        | Mid    | T2 で JSON 側は `instruments` に統一。`scenario.py` の normalize は legacy `.py` 用に残す  |
| Open Strategy 後に元 sidecar を外部編集 → cache と乖離                                     | Low        | Low    | `.py` cache の挙動と同じ。docs に記載（再 Open で同期）                                       |

---

## 9. 設計判断ログ

実装中に決まった判断をここに追記。

### D1: 命名は `<strategy>.json`（既存 layout サイドカーと同じ）

既に layout サイドカー機構が `<strategy>.json` で動いている。SCENARIO 用に新しい命名を導入すると 1 戦略あたりのファイル数が増えて煩雑。1 ファイル / トップレベルキーで責務分離する方が運用がシンプル。

### D2: 1 つの JSON に `scenario` キー + `viewport` / `windows` / ... を同居

レイアウト側コードと SCENARIO 側コードは互いの担当キーのみを触る。passthrough 用に `serde_json::Value` でもう一方のキーを保持する仕組みを入れる（破壊防止）。

### D3: `extract()` は廃止せず温存

外部スクリプトとの後方互換のため。新規コードは `load_scenario` を使う。

### D4: Rust 側に `.py` フォールバックを入れない

文字列スキャンを残すと将来の地雷。「サイドカー JSON 前提」を Rust GUI の規約とし、外部戦略の場合は手動で `.json` を作る運用にする。

### D5: LIVE_SCENARIO は本 Phase 対象外

Phase 8 と密結合のため `.py` 内に残置。同じ JSON に `live_scenario` キーを足す設計は Phase 8 で決める。

---

## 10. Open questions

- Q1: 外部 blacksheep 戦略向けに `python -m engine.strategy_runtime.migrate_scenario <strategy.py>` を提供するか？
  - 案: 不要（手動でユーザーが書く方が誤変換リスク低い）
- Q2: `<strategy>.json` の `scenario` キーを Strategy Editor 上で編集する UI は必要か？
  - 案: Phase 7.4 以降。本 Phase はフォーマット移行のみ。

---

## 11. 参考

- 既存 plan: [`Phase 7 - Replay UI Integration.md`](Phase%207%20-%20Replay%20UI%20Integration.md)
- 関連 doc: [`docs/strategy-replay.md`](../strategy-replay.md)
- ソース: [`python/engine/strategy_runtime/scenario.py`](../../python/engine/strategy_runtime/scenario.py) / [`src/ui/scenario_parser.rs`](../../src/ui/scenario_parser.rs) / [`src/ui/layout_persistence.rs`](../../src/ui/layout_persistence.rs)
