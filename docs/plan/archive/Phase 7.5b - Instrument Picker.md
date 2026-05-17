# Phase 7.5b — Instrument Picker（Add UI + 全上場銘柄ピッカー + ListAllListedSymbols）

> 本書は Phase 7.5a (`Phase 7.5 - Instrument List Verification.md`) の続編。7.5a で意図的に非スコープとした「銘柄を UI から追加する経路」と、それを支える backend 新 RPC、Resource リネーム、Phase 8 計画書整合更新を扱う。
>
> 7.5a 完了基準（§6 全達成）と §8 / §9 / §10 を **前提**とする。7.5a で確立した「`InstrumentRegistry` が single source of truth、ScenarioMetadata と sidecar JSON は projection」「CacheOnly 編集（通常編集は cache sidecar、明示 Save / Run inline flush で元 sidecar）」「dirty/flush の revision 二段管理」「`Without<ChartInstrument>` による layout 隔離」「BOM strip 統一」「Run 直前 inline flush」は **一切壊さない**。
>
> ブランチ予定: `feature/7.5b-instrument-picker`（7.5a の `feature/7.5-instruments-scenario-driven` から派生 or main マージ後に main から派生）

---

## 0. ゴール（要件と非スコープ）

### 0.1 ユーザー原要件（7.5a §0.1 の #2 / #3 / #7-Add 経路）

| # | 要件 | 7.5a | 7.5b |
|---|---|---|---|
| 2 | `[+ Add]` ボタンと全上場銘柄ピッカー | — | ✅ |
| 3 | 全上場銘柄一覧 = `scenario.end` 取引日の銘柄 | — | ✅ |
| 7 | Add を sidecar JSON に書き戻す（Close は 7.5a 完了済） | — | ✅ |

7.5b 完了時点でユーザーが見える効果:

- サイドバー `Instruments` セクションの末尾に `[+ Add]` ボタンが描画される（`registry.editable == true` のときのみ enabled）。
- `[+ Add]` 押下で `InstrumentPicker` floating window が出現し、現在の `scenario.end` 取引日に上場している全銘柄を search box + scrollable list で提示する。
- ピッカーから銘柄を選ぶと `InstrumentRegistry.add(id)` 経由で 7.5a の dirty/flush chain に乗り、同 tick で Chart spawn + cache sidecar writeback + ScenarioMetadata 同期が完了する。
- 元 sidecar への反映は 7.5a と同じ Phase 7.3 CacheOnly writeback 方針: 通常編集時は cache のみ更新、`File > Save` / `Save As` または Run 直前 inline flush で元 sidecar に反映。
- `instruments_ref` を持つ sidecar では `[+ Add]` も visually disabled（既存 `[× row]` lock と同じ判定）。

### 0.2 確定事項（kickoff 前に決める = §0.5 に集約）

7.5a §8 末尾の「7.5b 着手前に決めるべきこと」を本書 §0.5 で確定する。実装着手は §0.5 全項目クローズ後。

### 0.3 非スコープ（7.5b で実装しない）

- **`instruments_ref` の参照解決と Add UI 統合** — Phase 8 で v3 schema 設計と合わせて扱う。7.5b では `instruments_ref` 検出 → lock 維持（7.5a 挙動）のみ。
- **多銘柄 Chart データ分離（R3）** — Phase 7.6 仮称 (`Phase 7.6 - Replay Startup Progress Window.md` とは別)。複数 Chart が同じ `TradingData` を描く制約は残る。
- **Chart 位置・サイズの layout 復元（R4）** — Phase 7.6 / 8 で `WindowLayout.instrument_id` 拡張時に対応。
- **Undo/Redo への registry mutate 連携（R8）** — 誤 Add は `[× row]` で取り消し、誤 Close は `[+ Add]` で再追加。
- **picker UI からの bulk add（複数選択）** — 1 クリック 1 銘柄に固定。bulk は v2 機能候補。
- **picker 内の銘柄メタ表示（名称・市場区分）** — `instrument_id` 文字列のみ表示。名称付与は Phase 8 で `ListedInfo` 統合時に。
- **backend 側の jquants `listed_info` 取得 RPC 化** — 7.5b では CSV / ネットワークから listed_info を取得しない。artifact miss 時に既存 Nautilus catalog の bar dir を走査するだけ。ネットワーク取得は Phase 8。
- **`InstrumentList` 削除** — リネーム + 構造拡張のみ。旧 `ListInstruments` RPC 自身は当面 deprecated 扱いで残す（Phase 8 で削除判断）。

---

### 0.4 スコープに入れる確定事項

- **新 gRPC `ListAllListedSymbols(end_date)`**: 7.5a と違い backend 側 proto / Python / Rust client を **新規追加**する。proto 再生成事故を避けるため、`engine_pb2_grpc.py:6` の absolute-import 罠（memory `proto-regen-absolute-import-trap`）に従い `PYTHONPATH=$PWD\python\engine\proto` を verify ステップに含める。
- **`InstrumentList` → `AvailableInstruments` リネーム + 構造変更**: 旧 `InstrumentList { ids, loaded, error }` を `AvailableInstruments { by_end_date: HashMap<NaiveDate, Vec<String>>, in_flight: HashSet<NaiveDate>, last_error: Option<(NaiveDate, String)> }` に置換。`by_end_date` は UI セッション内の表示ミラーに留め、永続 cache は backend 側の `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` を正とする。`last_error` を `(NaiveDate, String)` にするのは、picker open 中の end_date と一致する失敗のみを当該 picker のエラー行として表示するため（§3.3）。
- **日付別銘柄一覧 artifact cache**: `ListAllListedSymbols(end_date)` はまず `artifacts/instrument-lists/listed-symbols-{end_date}.json` を読む。無ければ Nautilus catalog を一度だけ走査して当該 JSON を atomic write し、以降の picker open / アプリ再起動では artifact を読む。CSV は走査しない。
- **picker の表示順は `instrument_id` 昇順**（既存 `ListInstruments` の `sorted(seen)` と同じ規約）。
- **picker の searchbox**: 大文字小文字を無視した部分一致 filter。日本語名称は 7.5b では持たないので、コード文字列 (`1301.TSE` 等) に対する match のみ。
- **Add の dedup**: `InstrumentRegistry.add(id)` の既存仕様（dedup 戻り値 bool）に乗る。picker UI 側では追加済み銘柄を「Already added」と灰色化（クリック不可）。
- **Add 連打抑止**: picker クリック → registry mutate → mark_dirty → writeback の chain が同 tick で閉じる。同一銘柄の連打は `InstrumentRegistry.add(id)` の dedup に加え、UI 側で「同一 id の 100ms debounce」を入れる。別銘柄の連続 Add は妨げない。
- **`scenario.end` invalid format ハンドリング**: `parse_scenario_system` が `ScenarioMetadata.end` を `Option<String>` で持つため、picker open 時に `chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d")` で parse。失敗 or None なら picker を「`scenario.end` を設定してください」プレースホルダ表示のみとし、`ListAllListedSymbols` RPC は呼ばない。

---

### 0.5 着手前に確定する 4 項目（7.5a §8 末尾の引取り）

| # | 項目 | 確定方針 |
|---|---|---|
| Q1 | Code 正規化 5 桁 → 4 桁の規則 | 7.5a 現行の `code_to_instrument_id` を **無改修**で踏襲（`{code}.{venue}` 表記）。round-trip テストを 7.5b 単体テスト §5.1 に追加し fixture pin（`1301.TSE` ↔ `1301`、`13010.TSE` 5 桁先行ゼロ排除規則は **触らない**） |
| Q2 | `--jquants-csv-dir` と既存 catalog 引数の関係 | 7.5b では **新 `--jquants-csv-dir` は追加しない**。MVP の `ListAllListedSymbols` はまず `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` を読む。artifact miss 時だけ既存 `--jquants-catalog-path` または `DataEngine.last_replay_catalog_path` の Nautilus catalog を走査し、日付別 JSON を生成する。`--jquants-dir` は既存どおり replay 用 CSV → catalog 生成の入力であり、全銘柄列挙 RPC の直接入力にはしない。catalog 内 `data/bar/` が無いケースは `success=true, instrument_ids=[]` の artifact を作る |
| Q3 | universe API を新 `ListAllListedSymbols` 専用にするか / 既存 `ListInstruments(source=...)` に集約するか | **新 RPC として独立**させる。理由: (a) `ListInstruments` は「現在の replay session で使える銘柄」セマンティクスで Run 中に動的に変わる、(b) `ListAllListedSymbols` は「特定取引日の全上場銘柄カタログ」で session 非依存、(c) request に `end_date` 引数を追加するシグネチャ変更は backward compat を壊す。両者は別 RPC として共存させ Phase 8 で統合判断 |
| Q4 | v3 schema `instruments_ref` をどう解決して Add UI に統合するか | **7.5b では統合しない**。`instruments_ref` 検出時は picker 自体を visually disabled（7.5a の `[× row]` lock と同じ editable=false 判定で `[+ Add]` も封じる）。解決ロジック設計は Phase 8 へ繰越し、本書 §10 で別 issue 化 |

---

## 1. 7.5b で触るコード

| 領域 | ファイル | 種別 |
|---|---|---|
| proto | `python/proto/engine.proto` | 追記（既存 message 無改修） |
| backend RPC | `python/engine/server_grpc.py` | 新 handler + `__main__.py` 引数経路 |
| artifact cache | `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` | 日付別銘柄一覧 cache（生成物、既存 `.gitignore` の `artifacts/` 配下） |
| Python test | `python/tests/test_grpc_list_all_listed_symbols.py` | 新規 |
| Rust client | `src/main.rs` (`setup_backend_connection` の transport loop / `status_update_system` 周辺) | `ListAllListedSymbols` 呼び出し追加 |
| Rust resource | `src/trading.rs` | `InstrumentList` → `AvailableInstruments` リネーム + 構造変更 |
| transport | `src/trading.rs` の `TransportCommand` / `BackendStatusUpdate` | `FetchAvailableInstruments { end_date }` / `AvailableInstrumentsLoaded { end_date, ids }` / `AvailableInstrumentsFetchFailed { end_date, error }` 追加。`TransportCommand` は **既存 `TransportCommandSender { tx: mpsc::UnboundedSender<TransportCommand> }` Resource 経由で送る**（Bevy `EventWriter` ではない、`src/main.rs:164` 参照）|
| ピッカー UI | `src/ui/instrument_picker.rs` | **新規 module** |
| サイドバー | `src/ui/sidebar.rs` | `[+ Add]` 行追加、`editable=false` 時 disabled |
| 配線 | `src/ui/mod.rs`, `src/main.rs` | resource init / system 配線 |
| menu_bar | `src/ui/menu_bar.rs` | 既存 inline flush 経路は **無改修** |
| Phase 8 計画書 | `docs/plan/Phase 8 - Live Venue and Market Data.md` | sidebar universe 表を artifact cache / `AvailableInstruments` / `InstrumentRegistry` の関係に更新 |
| stub 再生成 | `python/engine/proto/engine_pb2*.py` | proto 再生成のたび `python/engine/proto/engine_pb2_grpc.py` を **relative import に手修正**（memory `proto-regen-absolute-import-trap`） |

7.5a で改修した `src/ui/components.rs` / `src/ui/scenario_parser.rs` / `src/ui/layout_persistence.rs` / `src/ui/floating_window.rs` / `src/ui/window.rs` は **無改修**（registry mutate API を picker から呼ぶだけ）。

---

## 2. データモデル

### 2.1 `AvailableInstruments`（`InstrumentList` 改名 + UI セッション内ミラー）

```rust
// src/trading.rs（旧 InstrumentList を置換）
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};

#[derive(Resource, Default, Debug, Clone)]
pub struct AvailableInstruments {
    /// end_date キーで全上場銘柄リストを保持する UI セッション内ミラー。
    /// 永続 cache は backend 側の artifacts/instrument-lists/*.json が正。
    pub by_end_date: HashMap<NaiveDate, Vec<String>>,
    /// 同一 end_date への並行 fetch 防止（picker open 連打対策）。
    pub in_flight: HashSet<NaiveDate>,
    /// 最後の fetch 失敗。picker 内のエラー行表示に使用。
    pub last_error: Option<(NaiveDate, String)>,
}
```

互換ポイント:
- 旧 `InstrumentList { ids: Vec<String>, loaded: bool }` の挙動（接続時に 1 度 `ListInstruments` を呼んで全 ids を保持）は **廃止**。代わりに 7.5b 接続時は `ListAllListedSymbols(end_date="")` を **呼ばない**。fetch は picker open 起点に切り替える。
- 同じ `scenario.end` で 2 回目以降 picker を開く場合は、まず `AvailableInstruments.by_end_date` を使う。アプリ再起動後や session miss では backend RPC を投げるが、backend 側が `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` を読むため Nautilus catalog 走査は発生しない。
- `src/main.rs:205-222` の `client.list_instruments(...)` 呼び出し path は **削除**（旧 `ListInstruments` RPC 自体は backend に残すが Rust client からは呼ばなくなる）。

構造ドリフト防止:
- `AvailableInstruments` の Rust Resource は上記 3 field（`by_end_date` / `in_flight` / `last_error`）を正とする。
- `by_instrument: HashMap<InstrumentId, ...>` / `by_date: HashMap<NaiveDate, ...>` の双方向 index は **採用しない**。7.5b の picker は「日付 → Vec<instrument_id>」の単方向 lookup だけで足り、instrument 起点の逆引きは `InstrumentRegistry.contains(id)` で判定する。
- `loaded: bool` / `error: Option<String>` は旧 `InstrumentList` の接続時一括ロード前提の state なので **復活させない**。日付別状態は `by_end_date.contains_key(d)` / `in_flight.contains(d)` / `last_error == Some((d, _))` で表現する。
- backend artifact JSON 側も `instrument_ids: Vec<String>` を正とし、by_instrument / by_date の materialized index は保存しない。

### 2.1.1 日付別銘柄一覧 artifact cache（backend）

```json
// artifacts/instrument-lists/listed-symbols-2024-01-04.json
{
  "schema_version": 1,
  "end_date": "2024-01-04",
  "source": "nautilus_catalog",
  "catalog_path": "artifacts/jquants-catalog",
  "generated_at": "2026-05-17T00:00:00Z",
  "instrument_ids": ["1301.TSE", "7203.TSE", "9984.TSE"]
}
```

- path は `artifacts/instrument-lists/listed-symbols-{YYYY-MM-DD}.json` 固定（生成物。repo 既存 `.gitignore` の `artifacts/` 配下なので git 管理外）。
- write は 7.5a の atomic write 方針と同じく、同ディレクトリ tmp → rename。
- 読み込み時は `schema_version == 1`、`end_date` 一致、`instrument_ids` が list[str] であることを検証する。不正なら artifact miss として catalog 再走査 + 上書き。
- artifact は手動で消せば再生成される。7.5b では Refresh ボタンや TTL invalidation は持たない。

### 2.2 `InstrumentPickerState` Resource（新規）

```rust
// src/ui/instrument_picker.rs
#[derive(Resource, Default, Debug, Clone)]
pub struct InstrumentPickerState {
    pub visible: bool,
    pub end_date: Option<NaiveDate>,   // 開く瞬間の scenario.end snapshot
    pub query: String,                  // searchbox 入力
    pub last_opened_at: Option<Instant>,// `[+ Add]` open 連打 debounce 用（100ms）
    pub last_added: Option<(String, Instant)>, // 同一銘柄 Add 連打 debounce 用（100ms）
}
```

### 2.3 Picker Window マーカー Component

```rust
#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerWindow;   // root に貼る

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerSearchBox;

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerListContainer;

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerRow {
    pub instrument_id: String,
    pub already_added: bool,
}

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerAddButton {
    pub instrument_id: String,
}
```

Picker window 自体は `WindowRoot` を持つ通常の floating window として spawn する（既存 `floating_window.rs` の drag / close observer に乗る）。**`ChartInstrument` は付けない**（layout には保存しない方針: §3.6 で `Without<InstrumentPickerWindow>` フィルタ追加）。

### 2.4 Transport 追加

```rust
// src/trading.rs::TransportCommand
FetchAvailableInstruments { end_date: NaiveDate },

// src/trading.rs::BackendStatusUpdate
AvailableInstrumentsLoaded { end_date: NaiveDate, ids: Vec<String> },
AvailableInstrumentsFetchFailed { end_date: NaiveDate, error: String },
```

既存の `InstrumentsLoaded { ids }` / `InstrumentLoadFailed { error }` は **7.5b で削除**（旧 `ListInstruments` 経路廃止に伴う）。

---

## 3. システム設計

### 3.1 picker open trigger

サイドバー `[+ Add]` ボタン押下時:

```rust
fn handle_add_button_pressed_system(
    mut picker: ResMut<InstrumentPickerState>,
    registry: Res<InstrumentRegistry>,
    scenario_meta: Res<ScenarioMetadata>,
    transport: Res<TransportCommandSender>,   // mpsc::UnboundedSender 経由（既存 7.5a と同じ）
    mut available: ResMut<AvailableInstruments>,
    // ... button query ...
) {
    if !registry.editable { return; }              // 7.5a editable lock 踏襲
    if debounce_active(&picker) { return; }        // 100ms

    let end_date = parse_scenario_end(&scenario_meta);  // None なら placeholder 表示で open
    picker.visible = true;
    picker.end_date = end_date;
    picker.query.clear();
    picker.last_opened_at = Some(Instant::now());

    if let Some(d) = end_date {
        if !available.by_end_date.contains_key(&d) && !available.in_flight.contains(&d) {
            available.in_flight.insert(d);
            let _ = transport.tx.send(TransportCommand::FetchAvailableInstruments { end_date: d });
        }
    }
}
```

### 3.2 backend fetch system

`src/main.rs::status_update_system` で `BackendStatusUpdate::AvailableInstrumentsLoaded` / `Failed` を受け取り、`AvailableInstruments` を更新 + `in_flight` から remove。

backend 送信側は `src/main.rs::setup_backend_connection` 内の transport loop に新 arm を追加し、tokio task で `client.list_all_listed_symbols(...)` を呼び出して、結果を status_tx 経由で UI に返す。既存の `list_instruments` 呼び出しブロック (`src/main.rs:205-222`) は **削除**し、connect 時の自動 fetch をやめる。

### 3.3 picker rendering system

```rust
fn render_picker_system(
    mut commands: Commands,
    picker: Res<InstrumentPickerState>,
    available: Res<AvailableInstruments>,
    registry: Res<InstrumentRegistry>,
    window_q: Query<Entity, With<InstrumentPickerWindow>>,
    // ... existing children query ...
) {
    if !picker.is_changed() && !available.is_changed() && !registry.is_changed() { return; }

    if !picker.visible {
        // close: despawn picker window if any
        for e in &window_q { commands.entity(e).despawn_recursive(); }
        return;
    }

    // visible: ensure window spawned, then rebuild list rows based on query + available + registry
    ensure_picker_window_spawned(&mut commands, &picker, &window_q);
    rebuild_picker_rows(&mut commands, &picker, &available, &registry);
}
```

Row state:
- `available.by_end_date[end_date]` を query filter で絞り込み → 各銘柄について `registry.contains(id)` なら `already_added=true` で row 灰色化。
- `available.last_error` が当該 end_date なら error 行表示。
- `available.in_flight.contains(end_date)` なら spinner 行表示。
- `end_date.is_none()` なら「`scenario.end` を設定してください」placeholder 行のみ。

### 3.4 picker row click → registry.add

```rust
fn handle_picker_add_click_system(
    mut registry: ResMut<InstrumentRegistry>,
    mut picker: ResMut<InstrumentPickerState>,
    // button query for Pressed events ...
) {
    if !registry.editable { return; }
    for ev in pressed_events {
        let id = &ev.instrument_id;
        if same_id_debounce_active(&picker, id) { continue; }
        if registry.add(id) {
            picker.last_added = Some((id.clone(), Instant::now()));
            // 7.5a chain が同 tick で writeback + Chart spawn + ScenarioMetadata sync
            // picker は閉じずに連続 add を許可（v2: ESC で close）
        }
    }
}
```

**重要**: ここで `mark_registry_dirty_system` は触らず、`registry.is_changed()` 経由で revision inc に任せる（7.5a §3.3 既存挙動を踏襲）。

### 3.5 picker close

- 外側クリック / ESC / 右上 `[×]` で `picker.visible = false`。
- close 時に backend fetch は cancel しない（次回 open で cache hit を期待）。
- `last_error` は close 時点で `None` に reset（次 open でクリーン状態）。

### 3.6 layout 隔離追加（7.5a の `Without<ChartInstrument>` と同じパターン）

7.5a で `Without<ChartInstrument>` を `build_layout` / `build_layout_for_explicit_save` / `apply_cache_restore_system` / `handle_save_layout_system` / `handle_save_as_layout_system` / `apply_layout_system` / `apply_pending_layout_system` / `save_layout_on_window_close` の panels Query に入れた（`src/ui/layout_persistence.rs` 内 **非 test 8 経路** + テスト内 1 経路 1451 行）。7.5b では picker window も同様に layout 管理外に置く。

実装案: marker を 1 つに統合する `LayoutExcluded` empty component を新設し、`ChartInstrument` 付与時と `InstrumentPickerWindow` 付与時の両方で `LayoutExcluded` も付ける。既存 非 test 8 経路 + test 1 経路（計 9 occurrences）の filter を `Without<ChartInstrument>` から `Without<LayoutExcluded>` に置換。

⚠️ **既存テスト**: 7.5a の `test_build_layout_excludes_chart` / `test_apply_layout_does_not_despawn_chart_when_layout_lacks_chart` 等が `Without<ChartInstrument>` の挙動を assert している。filter 置換に伴い、これらが `Without<LayoutExcluded>` 経由でも通ることを確認する追加 assert を入れる。filter リネーム単独 commit で test PASS を確認してから picker 実装に進む。

### 3.7 instruments_ref locked sidecar

- `registry.editable == false` のとき `[+ Add]` ボタン自体を visually disabled（既存 sidebar `[× row]` lock と同じ判定）。
- ボタン押下 → 早期 return（`handle_add_button_pressed_system` 冒頭の `if !registry.editable { return; }`）。
- picker が既に開いている状態で別 sidecar を Open して editable false になった場合、`picker.visible = false` に強制 close する追加 system を入れる（`registry.is_changed() && !registry.editable` で trigger）。

### 3.8 schedule 順序（Bevy `Update`）

7.5a chain に picker 系を挿入:

```
parse_scenario_system
  → sync_registry_from_scenario_loaded_system
  → sync_registry_from_scenario_cleared_system    （9.2 既存）
  → handle_add_button_pressed_system              （新規: picker open trigger）
  → handle_picker_add_click_system                （新規: registry mutate）
  → mark_registry_dirty_system                    （既存）
  → sync_scenario_metadata_from_registry_system   （7.5a §7.1）
  → writeback_scenario_instruments_system         （既存）
  → instrument_chart_sync_system                  （既存）
  → force_close_picker_on_lock_system             （新規）
  → render_picker_system                          （新規: picker UI rebuild、last 寄り）
```

`render_picker_system` は registry / available / picker の `is_changed()` で trigger するため、registry mutate と同 tick 内で「Add 済み」灰色化が反映される。
`force_close_picker_on_lock_system` は render より前に置き、別 sidecar Open で `editable=false` になった tick に picker を再描画せず閉じる。

---

## 4. backend (Python) 設計

### 4.1 proto

```protobuf
// python/proto/engine.proto に追記
message ListAllListedSymbolsRequest {
  string token = 1;
  // ISO-8601 (YYYY-MM-DD)。空文字なら "today" 扱い（catalog の最新日）。
  string end_date = 2;
}

message ListAllListedSymbolsResponse {
  bool success = 1;
  repeated string instrument_ids = 2;
  string error_message = 3;
  // 実際に解決された end_date（"today" 解決時の確認用）
  string resolved_end_date = 4;
}

service DataEngine {
  // ... 既存 ...
  rpc ListAllListedSymbols(ListAllListedSymbolsRequest) returns (ListAllListedSymbolsResponse);
}
```

### 4.2 handler

`python/engine/server_grpc.py` に `ListAllListedSymbols` handler を追加。実装方針:
- token check（既存と同じ pattern）。
- `end_date == ""` は catalog 内で観測できる最新 bar 日付に解決する。`end_date` 指定ありなら ISO date parse する。
- `resolved_end_date` 決定後、まず `artifacts/instrument-lists/listed-symbols-{resolved_end_date}.json` を読む。valid なら catalog を走査せず、その内容を返す。
- artifact miss / invalid の場合のみ、`catalog_path = self.engine.last_replay_catalog_path or self.engine._jquants_catalog_path` を base に取得（現行 `ListInstruments` と同じ優先順）。`--jquants-dir` は直接読まない。
- Nautilus catalog の現行 layout は `data/bar/<bar_type>/...parquet`（例: `1301.TSE-1-MINUTE-LAST-EXTERNAL`）であり、`data/bar/<YYYY-MM-DD>/` ではない。MVP 実装は bar type ディレクトリ名から既存 `ListInstruments` と同じ正規表現で instrument_id を抽出し、可能なら parquet timestamp/statistics で `resolved_end_date` 以下の bar 有無を判定する。timestamp 判定が取れない catalog では「ディレクトリ存在 = listed とみなす」近似に fallback し、厳密な listed_info CSV 解釈は Phase 8 へ送る。
- 走査結果は `instrument_id` 昇順に sort + dedup し、`artifacts/instrument-lists/listed-symbols-{resolved_end_date}.json` へ atomic write してから response する。
- `resolved_end_date` が catalog の最初の bar より前で対象銘柄が 0 件なら `success=true, instrument_ids=[]` の artifact を作る。catalog path が無く、artifact も無い場合は `success=false, error_message="No catalog_path available"`。

### 4.3 pytest

`python/tests/test_grpc_list_all_listed_symbols.py` 新規:
- token 不正で UNAUTHENTICATED。
- `end_date=""` + catalog ありで `success=true`, `instrument_ids != []`, `resolved_end_date != ""`、日付別 artifact が生成される。
- `end_date="2024-01-04"` で当日 artifact miss → catalog 走査 → artifact write → success。
- `end_date="2024-01-04"` で artifact hit → catalog path 未設定でも success（catalog を走査しない証跡）。
- invalid artifact（schema mismatch / end_date mismatch / 壊れた JSON）は miss 扱いで再生成。
- catalog 最初の bar より前の日付で `success=true, instrument_ids=[]`、空 artifact が生成される。
- `end_date="2099-12-31"` のような未来日付は、catalog 内の最新 bar 日付以下に存在する銘柄を返し、`resolved_end_date` は catalog 最新日になる。
- artifact miss かつ catalog 未設定で `success=false`。

### 4.4 proto 再生成手順（罠回避）

memory `proto-regen-absolute-import-trap` 必読。

```powershell
# 1. 再生成
cd python
python -m grpc_tools.protoc -I=proto --python_out=engine/proto --grpc_python_out=engine/proto proto/engine.proto

# 2. engine_pb2_grpc.py:6 の import を必ず relative に手修正
#    × import engine_pb2 as engine__pb2
#    ○ from . import engine_pb2 as engine__pb2

# 3. backend smoke test
$env:PYTHONPATH = "$PWD\engine\proto"
python -m engine --port 50051 --jquants-catalog-path <catalog-path>
```

CI / pre-commit hook で grep check を追加する（`engine_pb2_grpc.py:6` が `from . import` で始まることを確認）。

---

## 5. テスト

### 5.1 Rust 単体

#### Picker open / close
- `test_picker_opens_on_add_button_pressed`: button event → `picker.visible=true`, end_date snapshot 取得。
- `test_picker_skips_open_when_registry_locked`: `editable=false` → `picker.visible` 不変。
- `test_picker_skips_open_during_debounce`: 100ms 以内の 2 回目押下 → 1 度しか open しない。
- `test_picker_force_close_on_lock`: open 中に別 sidecar Open で `editable=false` → 強制 close。

#### Fetch transport
- `test_picker_open_dispatches_fetch_when_cache_miss`: `available.by_end_date` に未登録 → `FetchAvailableInstruments` event 発行 + `in_flight` insert。
- `test_picker_open_skips_fetch_on_session_cache_hit`: 同 end_date が `AvailableInstruments.by_end_date` に既にある → fetch 投げない。
- `test_picker_open_skips_fetch_when_in_flight`: 並行 fetch 中 → 2 度目は投げない。
- `test_available_loaded_clears_in_flight`: `AvailableInstrumentsLoaded` 受信 → `in_flight` から remove、`by_end_date` 更新。
- `test_available_failed_sets_last_error`: failed 受信 → `last_error` セット、`in_flight` から remove。

#### Add chain（7.5a 連動）
- `test_picker_click_adds_to_registry`: row click → `registry.contains(id)==true`、Chart 1 entity spawn、cache sidecar JSON 更新（7.5a writeback chain）。
- `test_picker_click_is_idempotent_for_already_added`: 既追加銘柄を再 click → registry 不変、Chart 増えない、writeback revision 不変。
- `test_picker_click_debounces_same_id_only`: 同一 id の 100ms 内再 click は無視、別 id の click は通る。
- `test_picker_click_blocked_when_locked`: `editable=false` で row click → 何も起きない。

#### Q1 round-trip
- `test_code_to_instrument_id_round_trip_4_digit`: `1301` ↔ `1301.TSE` pin。
- `test_code_to_instrument_id_round_trip_5_digit`: 5 桁先行ゼロ排除規則の現状挙動を pin（**規則変更しない**ことの証跡）。

#### Resource rename 互換
- `test_available_instruments_replaces_old_instrument_list`: 旧 `InstrumentList` を直接参照する system が 0 件であること（`cargo check` で coverage）。
- `test_available_instruments_shape_does_not_reintroduce_old_or_bidirectional_state`: `AvailableInstruments` に `loaded` / `error` / `by_instrument` / `by_date` を追加しないことを compile-time API usage で pin（テスト helper は `by_end_date` / `in_flight` / `last_error` のみを構築）。

### 5.2 Rust 統合（Bevy schedule）

- `test_e2e_open_then_add_via_picker`: pair_trade_minute.json open → 2 銘柄 + 2 Chart → `[+ Add]` → fake transport が `FetchAvailableInstruments` を受信 → `AvailableInstrumentsLoaded { ids: ["1301.TSE","7203.TSE","9984.TSE"] }` 模擬注入 → picker に 3 行表示 (2 件 Already added: `1301.TSE` / `7203.TSE`、1 件 addable: `9984.TSE`) → `9984.TSE` click → registry 3 銘柄、Chart 3 entity、cache sidecar `["1301.TSE","7203.TSE","9984.TSE"]`。
- `test_e2e_add_then_run`: 上記後 Run → backend 受信 RunStrategy の instruments が 3 銘柄、両 sidecar 同期（7.5a Run inline flush の互換維持確認）。
- `test_e2e_picker_no_open_for_invalid_end_date`: scenario.end が空 / invalid → picker 開けるが placeholder のみ、fetch 投げない。
- `test_e2e_layout_excludes_picker_window`: picker open 中に layout save → picker window が `WindowLayout` JSON に含まれない、reopen で layout 復元しても picker window が誤 spawn されない。

### 5.3 Python pytest

§4.3 参照。

### 5.4 手動 E2E

7.5a §5.3 の延長として:

1. `pair_trade_minute.json` Open → Instruments 2 銘柄 + `[+ Add]` ボタン可視 + Chart 2。
2. `[+ Add]` → picker open、3 銘柄以上のリスト表示（catalog 依存）、`1301.TSE` / `7203.TSE` は灰色 (Already added)。
3. 新銘柄を click → 即座にサイドバー追加 + 新 Chart spawn + cache sidecar 更新。元 sidecar は不変（CacheOnly writeback、明示 Save / Run inline flush で反映）。
4. picker 開いたまま searchbox に `9984` 入力 → 該当行のみ残る。
5. picker 閉じる → 同セッションの再 open は `AvailableInstruments.by_end_date` から即表示。アプリ再起動後の再 open は backend が `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` を読んで返す。
6. `instruments_ref` locked sidecar Open → `[+ Add]` が disabled、警告行は 7.5a 表示そのまま。
7. backend 落ちている状態で `[+ Add]` → fetch failed → picker 内に error 行表示、retry は close → reopen で再投。
8. Run 実行 → 追加銘柄が `RunStrategy` config に乗る（backend ログで確認）。

---

## 6. 実装ステップ

### Step 1 — `AvailableInstruments` リネーム + 旧 `ListInstruments` 呼び出し撤去
- `InstrumentList` → `AvailableInstruments` 構造変更。
- `AvailableInstruments` の field は §2.1 の `by_end_date` / `in_flight` / `last_error` に固定。Navigator 提案の `by_instrument` / `by_date` 双方向 index、旧 `loaded` / `error` は実装しない。
- `src/main.rs:205-222` の `list_instruments` ブロック削除、`BackendStatusUpdate::InstrumentsLoaded` / `InstrumentLoadFailed` 削除。
- 既存 sidebar の旧 `Instruments` 表示（`AvailableInstruments` 経由のもの）が無いことを確認（7.5a で sidebar は `InstrumentRegistry` 駆動に置換済み）。
- `cargo check` warning 0、`cargo test --lib` 回帰なし。

### Step 2 — proto + backend `ListAllListedSymbols`
- proto 追記 → 再生成 → relative import 手修正。
- `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` の read / validate / atomic write helper を追加。
- handler 実装 + pytest §5.3。
- backend を `cargo run` 抜きで smoke test（grpcurl or python client）。

### Step 3 — Rust client + transport 追加
- `TransportCommand::FetchAvailableInstruments` / `BackendStatusUpdate::AvailableInstrumentsLoaded / Failed` 追加。
- backend channel async task で `list_all_listed_symbols` 呼び出し、結果を status_tx 経由で UI へ。Rust 側は artifact path を意識せず、backend response を `AvailableInstruments.by_end_date` に入れるだけ。
- `AvailableInstruments` 更新経路を `status_update_system` に追加。
- 単体テスト §5.1「Fetch transport」5 件。

### Step 4 — picker window + sidebar `[+ Add]` ボタン
- `src/ui/instrument_picker.rs` 新規。
- `InstrumentPickerState` resource、marker components、render system、open/close handler、debounce。
- sidebar `Instruments` セクション末尾に `[+ Add]` 行追加（`editable=true` で enabled、`editable=false` で disabled）。
- `LayoutExcluded` empty component 導入 + 既存非 test 8 経路 (+test 1 経路) filter 置換、ChartInstrument / InstrumentPickerWindow 両方で付与。
- 単体テスト §5.1「Picker open / close」4 件 +「Layout 隔離」2 件。

### Step 5 — Add click chain（7.5a 連動検証）
- `handle_picker_add_click_system` 実装、registry.add 経由。
- 単体テスト §5.1「Add chain」3 件、統合テスト §5.2 全 4 件。
- 手動 E2E §5.4 全 8 項目。

### Step 6 — Q1 round-trip + Resource rename 回帰
- §5.1「Q1 round-trip」「Resource rename 互換」追加。
- `cargo test --lib` 全 PASS。

### Step 7 — Phase 8 計画書更新
- `docs/plan/Phase 8 - Live Venue and Market Data.md` の Sidebar universe ソース表を「`artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` + `AvailableInstruments`（日付別全上場銘柄 cache の UI ミラー、picker fetch 起点）+ `InstrumentRegistry`（scenario JSON 駆動の選択済み）」に書き換え。
- 旧 `ListInstruments` の Phase 8 削除判断ロジを追記（live venue 起動時に必要なら残す / 不要なら deprecation 期間後に削除）。

---

## 7. 完了基準

- ✅ `[+ Add]` 押下で picker が end_date 付きで開く、`scenario.end` 未設定 / invalid は placeholder。
- ✅ picker 内クリックで 7.5a chain が動き、Chart spawn + cache sidecar 更新 + ScenarioMetadata 同期が同 tick で完了。
- ✅ 元 sidecar は CacheOnly writeback 方針通り、明示 Save または Run inline flush で初めて更新。
- ✅ `instruments_ref` locked sidecar では `[+ Add]` も disabled、誤 click も無効。
- ✅ 同一銘柄の Add 連打 (100ms 内) は debounce で 1 回扱い、`registry.add` の dedup と二重で安全。別銘柄の連続 Add は許可。
- ✅ 新 `ListAllListedSymbols` RPC が §4.3 の pytest 全件 PASS、proto 再生成後の relative import check PASS。
- ✅ `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` が日付別に生成され、2 回目以降は artifact hit で catalog 走査なしに返る。
- ✅ 旧 `ListInstruments` 経由の自動 fetch が削除され、connect 時 RPC 呼び出しが 0 件。
- ✅ `InstrumentList` → `AvailableInstruments` リネーム、全参照置換、`cargo check` warning 0。
- ✅ `AvailableInstruments` に `by_instrument` / `by_date` / `loaded` / `error` が再導入されていない。
- ✅ §5.1 / §5.2 / §5.3 / §5.4 全件 PASS。
- ✅ 7.5a の §6 完了基準が **回帰していない**こと（pair_trade_minute.json Open → 2 銘柄 + 2 Chart、Close → cache 更新、etc.）。
- ✅ Phase 8 計画書の sidebar universe 表が更新されている。

---

## 8. リスク・既知の制約

| # | 項目 | 対応 |
|---|---|---|
| R10 | `LayoutExcluded` への filter 置換で 7.5a テスト群が回帰 | Step 4 を「filter リネーム単独 commit → test PASS → picker 追加」の 2 段に分け、リネーム単独で全 7.5a テスト PASS を確認 |
| R11 | proto 再生成のたび absolute import が戻り backend `grpc: ERR` | pre-commit hook or CI で `engine_pb2_grpc.py:6` の `from .` を grep check（memory `proto-regen-absolute-import-trap`） |
| R12 | picker fetch failed 時の retry UX | 7.5b は「close → reopen で再投」のみ。明示 retry ボタンは v2 候補 |
| R13 | catalog `data/bar/` の銘柄抽出が時間軸的に粗い | 7.5b MVP は「該当日付以前に上場している全銘柄」の近似で OK。結果は日付別 artifact に固定されるため、厳密な listed_info CSV 解釈へ切替えた際は artifact 再生成方針も Phase 8 で決める |
| R14 | picker open 中の registry mutate 競合 | render_picker_system が registry / available / picker の `is_changed()` を OR 監視するため、同 tick 内で Add 済み灰色化が反映。race なし |
| R15 | 100ms debounce が画面 DPI / 入力デバイスで体感差 | 必要なら user setting 化を Phase 7.6 以降で検討。7.5b は固定 |
| R16 | bevy `add_systems` タプル 20 上限 | 7.5b で 5 system 追加。`UiPlugin::build` の `add_systems(Update, (...))` を必要に応じて分割呼び出しに（memory bevy-engine スキル）|
| R17 | stale artifact により catalog 更新後も古い銘柄一覧を返す | 7.5b は「artifact を消せば再生成」で割り切る。Refresh / TTL / catalog hash invalidation は Phase 8 以降 |

---

## 9. ロールアウトと回帰防止

### 9.1 マージ前チェックリスト

- `cargo test --lib` 全 PASS（7.5a の 128 件 + 7.5b 新規分）
- `cargo check` warning 0
- `python -m pytest python/tests/test_grpc_list_all_listed_symbols.py` PASS
- `artifacts/instrument-lists/` の生成物がテスト後に git dirty として残っていないこと（必要なら test tempdir / cleanup）
- proto stub の `from .` import grep check PASS
- 手動 E2E §5.4 全 8 項目 PASS
- 7.5a fixture (`pair_trade_minute.json` / `e2e_v1_sidecar.json` / `e2e_instruments_ref_locked.json`) が E2E 副作用で書き換わっていないこと

### 9.2 回帰チェック想定（Phase 8 以降）

- `InstrumentRegistry` single source of truth、ScenarioMetadata / cache sidecar / 元 sidecar（明示 Save または Run inline flush 後）が projection である不変条件
- dirty/flush revision 二段管理（picker add でも同じ chain）
- `LayoutExcluded` filter による layout 隔離（Chart + Picker 両方）
- Run 直前 inline flush（picker add 直後 Run でも sidecar 確定）
- BOM 耐性（picker 経由で書く JSON も BOM なし）
- 日付別 artifact cache は `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` に限定し、通常の strategy cache sidecar (`app_state.json`) と混ぜない

### 9.3 Phase 8 連携

- v3 schema `instruments_ref` 解決は **Phase 8** で扱う。本書 §0.3 で繰越明示。
- live venue 接続時に `AvailableInstruments` を venue 由来 listed_info で埋める場合、`FetchAvailableInstruments { end_date }` の handler を venue 別に切替（adapter pattern）。その際、Replay catalog 由来の `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` と Live venue 由来 cache を同じファイル名で混ぜない（Phase 8 で `source` / venue 別 path を設計）。
- 旧 `ListInstruments` RPC は Phase 8 の live venue 計画完了後に削除可否を再判断。それまで proto / backend は残置。

---

## 10. 別 issue として記録（Phase 8 以降）

- `instruments_ref` 参照解決の v3 schema 設計（Phase 8）。
- 多銘柄 Chart データ分離（R3、Phase 7.6 仮称）。
- Chart 位置・サイズ復元（R4、Phase 7.6 / 8）。
- picker UI bulk add（v2）。
- picker UI に銘柄名称表示（Phase 8、`ListedInfo` 統合時）。
- backend `listed_info` 取得を CSV から jquants ネット fetch に切替（Phase 8）。

---

## 11. Phase 7.5a からの依存差分（読み手向け要約）

7.5a と比べて変わるのは:

1. **registry mutate 経路が +1 増える**（既存: 外部 JSON edit + Open、Chart `[×]`、sidebar `[× row]` ／ 追加: picker click）。
2. **新 backend RPC `ListAllListedSymbols`** が 1 本増える。proto / Python / Rust client 全部触る。
3. **日付別 artifact cache** が `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` に増える。picker open は memory miss → artifact hit → catalog scan の順。
4. **Resource 名 `InstrumentList` → `AvailableInstruments`** に rename + 構造拡張（HashMap<end_date, ids>）。
5. **layout 隔離 marker が `ChartInstrument` から `LayoutExcluded`** に汎化。Chart も Picker も同じ filter で除外。
6. **connect 時の自動 `ListInstruments` 呼び出しを停止**。Universe fetch は picker open trigger に変更。

7.5a の dirty/flush chain、CacheOnly writeback、ScenarioMetadata 同期、Run inline flush、BOM strip、`Without<LayoutExcluded>` を使った layout 隔離 は **そのまま再利用**するため、本書での再記述はしない（7.5a §3 / §7 参照）。
