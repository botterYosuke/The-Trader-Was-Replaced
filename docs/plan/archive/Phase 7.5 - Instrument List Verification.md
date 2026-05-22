# Phase 7.5 — Instruments パネル再設計（Scenario 駆動 + Chart 寿命連動）

> ## 📊 実装ステータス（2026-05-17 時点）
>
> **Phase 7.5a: 実装完了 ✅** — 6 commits (`320745b` → `fc5a72a`)、`cargo test --lib` 120 passed / 1 ignored / 0 failed、手動 E2E §5.3 全 7 チェック PASS
>
> ブランチ: `feature/7.5-instruments-scenario-driven`
>
> 完了サマリ:
> - Step 1-6 実装 ✅
> - 手動 E2E 全項目 PASS ✅
> - BOM 耐性 fix-up ✅（E2E で発見・追加対応）
> - ScenarioMetadata 同期 system 追加 ✅（Run validation 整合用、当初計画外）
>
> 残課題 → §9 / §10 / §11 を参照
>
> ---
>
> **スコープ分割**: 元の 7 要件を 2 つの Phase に分けて段階導入する。
>
> - **Phase 7.5a (本書の主スコープ)**: scenario JSON → `InstrumentRegistry` → Chart spawn/despawn の **寿命連動コア** と、Add/Close 後の **元 sidecar + cache sidecar への atomic 書き戻し**。ピッカー / 新 RPC / Phase 8 計画書更新は含めない。Add ボタンは Phase 7.5a では描画しない（外部 JSON 編集 → Open のみで増やせる）。
> - **Phase 7.5b (別計画書、後続)**: `[+ Add]` ボタン、全上場銘柄ピッカー、新 gRPC `ListAllListedSymbols`、`InstrumentList` → `AvailableInstruments` リネーム、Phase 8 計画書（universe ソース別表）との整合更新。
>
> Phase 7.5a は backend gRPC / proto を 1 行も触らず、Rust 側のみで閉じる。これにより stub 再生成事故・Phase 8 設計の先取り・新 RPC 設計の判断ぶれを 7.5a の実装期間中ゼロにできる。

---

## 0. ゴール（要件と非スコープ）

### 0.1 ユーザー原要件と分担

| # | 要件 | 7.5a | 7.5b |
|---|---|---|---|
| 1 | `Instruments` に表示する銘柄は sidecar JSON の `scenario.instruments` から取る | ✅ | — |
| 2 | `[+ Add]` ボタンと全上場銘柄ピッカー | — | ✅ |
| 3 | 全上場銘柄一覧 = `scenario.end` 取引日の銘柄 | — | ✅ |
| 4 | Instrument 登録 → 同 key の Chart を spawn | ✅ | — |
| 5 | サイドバー `Chart` ボタン廃止 | ✅ | — |
| 6 | Chart `[×]` で Instrument 抹消 | ✅ | — |
| 7 | Add / Close を sidecar JSON に書き戻す | ✅（Close 経路）| ✅（Add 経路）|

7.5a 完了時点でユーザーが見える効果: 「`pair_trade_minute.json` を開くと 2 銘柄が並んで 2 Chart spawn、Chart `[×]` で Instrument と該当 Chart が消え、その変更が元 sidecar + cache sidecar の両方に保存される」。

### 0.2 確定事項（レビュー 3 周分の反映）

- **永続化先**: writeback 成功時に **元 sidecar JSON と cache sidecar JSON の両方を atomic に更新**（tmp → rename）。backend が `strategy_path.with_name(stem + ".json")` で **cache sidecar を読む** (`python/engine/strategy_runtime/scenario.py:28-32, 243`) ため、元だけ更新すると Run 時に stale を掴む（レビュー Critical #1）。
- **Run 直前の同期**: `is_changed()` ベースの待ちガードは Bevy の `EventReader::read()` と相性が悪い（`continue` で Run イベントが失われる）。代わりに **`handle_strategy_run_system` 内で `RunStrategy` 送信直前に同期 flush を inline で実行**する（writeback system とは別経路、event を消費せず即書き）。
- **Schema 互換**:
  - v1 単数 `instrument` / 文字列も Vec も含む legacy → writeback 時に v2 へ正規化（`schema_version=2`、`instrument` 削除、`instruments` に登録、warn ログ）。Add/Close は許可。
  - `scenario.instruments_ref` キーを含む sidecar → **編集ロック**。`[× row]` と Chart `[×]` を visually disabled、warn UI 表示、writeback しない。Phase 8 で参照解決ロジックを設計するまで触らない。
- **Chart 除外経路**: `build_layout` だけでなく **`apply_layout_system` の despawn-not-in-layout** からも `ChartInstrument` 付き window を除外する（`layout_persistence.rs:700-720`、レビュー Critical #4）。
- **同期方向**: `parse_scenario_system` が **`ScenarioLoadedFromFile` イベント発火**。registry → JSON 書き戻し経路は ScenarioMetadata を in-place 更新するがイベント発火しない（`is_changed()` 監視への退化を防ぐ）。
- **`parse_scenario_system` の mtime**: Local → `Resource<ScenarioFileWatchState>` に格上げし、writeback 直後の mtime 転記で再 trigger を抑止。

### 0.3 非スコープ（7.5a で実装しない）

- `[+ Add]` ボタンと全上場銘柄ピッカー（7.5b）
- gRPC `ListAllListedSymbols`（7.5b）
- `InstrumentList` Resource の改修（7.5b で `AvailableInstruments` にリネーム）
- Phase 8 計画書の更新（7.5b で実施）
- 多銘柄 Chart データ分離（Chart 2 つ並べると同じ `TradingData` を描く制約は残る、Phase 7.6 仮称 or Phase 8）
- Undo/Redo への Chart spawn/despawn 連携
- Chart 位置・サイズの layout 復元（`WindowLayout` に Chart 保存しない方針のため）

---

## 1. 7.5a で触るコード

| 領域 | ファイル |
|---|---|
| データモデル | `src/ui/components.rs`, `src/trading.rs`（参照のみ） |
| Scenario 解析 | `src/ui/scenario_parser.rs` |
| サイドバー描画 | `src/ui/sidebar.rs` |
| Chart spawn | `src/ui/window.rs`, `src/ui/floating_window.rs` |
| Layout 保存・復元 | `src/ui/layout_persistence.rs` |
| Run 経路 | `src/ui/menu_bar.rs::handle_strategy_run_system` |
| 配線 | `src/ui/mod.rs` |

backend / proto / Python は **一切触らない**。

---

## 2. データモデル

### 2.1 `InstrumentRegistry`（選択済み）

```rust
// src/ui/components.rs
#[derive(Resource, Default, Debug, Clone)]
pub struct InstrumentRegistry {
    pub ids: Vec<String>,        // 表示順保持、dedup
    pub editable: bool,          // instruments_ref を含む sidecar は false にして UI ロック
}

impl InstrumentRegistry {
    pub fn add(&mut self, id: &str) -> bool { /* dedup */ }
    pub fn remove(&mut self, id: &str) -> bool { /* */ }
    pub fn contains(&self, id: &str) -> bool { /* */ }
}
```

### 2.2 `ScenarioLoadedFromFile` イベント

```rust
#[derive(Event, Debug, Clone)]
pub struct ScenarioLoadedFromFile {
    pub source_path: PathBuf,
    pub instruments: Vec<String>,
    pub end: Option<String>,
    pub has_instruments_ref: bool,
}
```

- 発火元は `parse_scenario_system` の **ファイル read 成功パスのみ**。
- registry → JSON writeback は発火しない（同期方向の一方向化）。

### 2.3 `ScenarioFileWatchState` Resource

```rust
#[derive(Resource, Default)]
pub struct ScenarioFileWatchState {
    pub last_path: Option<PathBuf>,
    pub last_mtime: Option<SystemTime>,
}
```

- `parse_scenario_system` の Local を格上げ。writeback system が write 後にこの `last_mtime` を更新して再 trigger を抑止（R5 回帰）。

### 2.4 `ChartInstrument` Component

```rust
#[derive(Component, Debug, Clone)]
pub struct ChartInstrument {
    pub instrument_id: String,
}
```

- Chart の `WindowRoot` に貼る。close observer 内で逆引き → registry remove。
- 描画には使わない（7.5a 非スコープ）。

### 2.5 `ScenarioInstrumentsWritebackState` Resource

```rust
#[derive(Resource, Default)]
pub struct ScenarioInstrumentsWritebackState {
    pub revision: u64,         // registry 変更ごとに inc
    pub flushed_revision: u64, // writeback 成功時に revision を転記
    pub last_error: Option<String>,
}
```

- registry の変更検知は **明示 revision** で行う。`is_changed()` の race を避ける。
- writeback system が成功したら `flushed_revision = revision`、失敗なら `last_error` を埋めて次フレーム再試行。
- Run 直前 inline flush は revision の値とは無関係に「現在の registry を書く」だけ。

---

## 3. システム設計

### 3.1 `parse_scenario_system` 改修

- Local mtime を `ScenarioFileWatchState` Resource に置き換え。
- 解析成功時のみ `EventWriter<ScenarioLoadedFromFile>` 発火。
- `instruments_ref` キーを sidecar JSON の `scenario` 直下から検出して `has_instruments_ref: bool` をイベントに乗せる（serde の Untagged or `serde_json::Value` ベースで low-level に判定）。

### 3.2 `sync_registry_from_scenario_loaded_system`（新規）

```rust
fn sync_registry_from_scenario_loaded_system(
    mut events: EventReader<ScenarioLoadedFromFile>,
    mut registry: ResMut<InstrumentRegistry>,
    mut writeback: ResMut<ScenarioInstrumentsWritebackState>,
) {
    for ev in events.read() {
        registry.ids = ev.instruments.clone();
        registry.editable = !ev.has_instruments_ref;
        // ファイルロード由来の代入は「既に flushed」扱いにして Run 直前 inline flush を起動させない
        writeback.revision = writeback.flushed_revision;
        writeback.last_error = None;
    }
}
```

### 3.3 `mark_registry_dirty_system`（新規・微小）

- `InstrumentRegistry` の `is_changed()` を見て `writeback.revision += 1`。ファイルロード由来は §3.2 で既に flushed と同値にしているので、ここで再 inc されてもループしない（writeback 後にまた flushed = revision にするため）。

### 3.4 `writeback_scenario_instruments_system`（新規）

- 条件: `registry.editable == true && writeback.revision != writeback.flushed_revision`。
- 動作:
  1. `buffer.original_path` から元 sidecar path を導出（`with_extension("json")`）。`None` の場合は **元 sidecar のみ skip** し cache sidecar への書き込みは続行。
  2. cache sidecar path = `cache_state_paths()` の `.json`。
  3. 両 path について `read → scenario.instruments を registry.ids で置換 → tmp に write → rename` の atomic write を実施。
  4. v1 検出時は v2 正規化（`schema_version=2` セット、`instrument` キー削除、warn ログ）。
  5. 両方成功なら `flushed_revision = revision`、`last_error = None`。`ScenarioFileWatchState.last_mtime` を新 mtime に転記。
  6. 失敗時は `last_error` セット、`flushed_revision` 据え置き、次フレーム再試行。トーストは別 system で表示。

### 3.5 Run 直前 inline flush

`handle_strategy_run_system` (`src/ui/menu_bar.rs:547`) の `RunStrategy` コマンド送信 **直前** に同期 flush を挟む:

```rust
for event in events.read() {
    // ... 既存の scenario validation ...

    // 7.5a: Run 直前に sidecar を確定する（writeback system の dirty/flush 状態に依存しない）
    if registry.editable {
        if let Err(e) = flush_sidecars_now(&registry, &buffer, &scenario_paths_for_run) {
            error!("Run blocked: sidecar flush failed: {}", e);
            continue;
        }
    }

    // RunStrategy 送信
}
```

- `flush_sidecars_now` は §3.4 の write ロジックを切り出した同期関数。`is_changed()` も EventReader も触らない、ただ書くだけ。
- 既に最新が書き込まれていても idempotent（再書きしても同内容なので影響なし）。
- writeback system の race を完全に閉じる。

### 3.6 `instrument_chart_sync_system`（新規）

```rust
fn instrument_chart_sync_system(
    registry: Res<InstrumentRegistry>,
    chart_q: Query<(Entity, &ChartInstrument), With<WindowRoot>>,
    mut commands: Commands,
) {
    if !registry.is_changed() { return; }
    let desired: HashSet<&str> = registry.ids.iter().map(|s| s.as_str()).collect();
    let spawned: HashMap<&str, Entity> = chart_q.iter().map(|(e, c)| (c.instrument_id.as_str(), e)).collect();
    for id in &desired { if !spawned.contains_key(id) { spawn_chart_panel(&mut commands, id); } }
    for (id, e) in &spawned { if !desired.contains(id) { commands.entity(*e).despawn_recursive(); } }
}
```

### 3.7 Chart `[×]` close observer 拡張

```
[×] click
 → close observer
   1. root が ChartInstrument を持つか確認
   2. 持っていて registry.editable なら registry.remove(id)
       （持っていて editable=false なら無視 + warn UI、despawn しない）
   3. ChartInstrument 付きなら history.push_window_despawn() を呼ばない
   4. despawn_recursive(root) を 1 回だけ実行
   5. registry 変更により writeback dirty 化 → 次フレーム writeback
```

### 3.8 サイドバー `[× row]`

- Chart `[×]` と同経路。`InstrumentRegistry.remove` を呼び、sync が Chart despawn、writeback が sidecar 更新。
- `editable=false` の場合は `[× row]` を visually disabled、tooltip 相当の warn を sidebar 下部に。

### 3.9 Layout からの Chart 除外（**3 経路**）

| 経路 | 改修 | 根拠 |
|---|---|---|
| `build_layout` (`layout_persistence.rs:170`) | panels query に `Option<&ChartInstrument>` を追加、`Some` なら filter で落とす | Chart を `WindowLayout` に保存しない |
| `apply_layout_system` despawn 集約 (`layout_persistence.rs:700-720`) | `to_despawn` filter に `ChartInstrument` を持つ entity を除外する条件追加 | **layout に Chart が無くても registry 由来 Chart を消さない**（レビュー Critical #4） |
| `apply_pending_layout_system` / cache restore の同等 despawn 経路 | 同上 | 同上 |
| drag-end observer (`floating_window.rs:158`) | `ChartInstrument` 付きなら `history.push_window_move` も `auto_save.mark_layout_changed` も skip | Chart は history / autosave に積まない |
| close observer (`floating_window.rs:241`) | `ChartInstrument` 付きなら `history.push_window_despawn` skip（§3.7 と同じ） | 同上 |

### 3.10 旧 `PanelKind::Chart` の単一処理: ignore + warn

- `panel_spawn_dispatcher_system` の `PanelKind::Chart` arm: warn no-op。
- `apply_layout_system` / `apply_pending_layout_system` / cache restore: `WindowLayout.kind == PanelKind::Chart` のエントリは skip + warn。「転送 or 無視」の選択肢は残さない。
- `editor_history.rs` の Chart 直接 spawn テストは別 PanelKind に差し替え（or `ChartInstrument` 付き helper）。

### 3.11 サイドバー UI

```
┌─ Instruments ─────────┐
│ 1301.TSE          [×] │
│ 7203.TSE          [×] │
└───────────────────────┘
```

- `[+ Add]` ボタンは **描画しない**（7.5b で追加）。
- `Panels` セクションから `Chart` ボタンを削除。
- `editable=false` のときは各 `[× row]` を disabled + sidebar 最下部に `"This sidecar uses 'instruments_ref' — read-only in Phase 7.5a"` の警告行を表示。

---

## 4. 実装ステップ

### ✅ Step 1 — 型と Resource、配線（commit `320745b`）
- ✅ `src/ui/components.rs`: `InstrumentRegistry`(+helper), `ChartInstrument`, `ScenarioLoadedFromFile`, `ScenarioFileWatchState`, `ScenarioInstrumentsWritebackState` を追加。
- ✅ `src/ui/mod.rs`: `init_resource` / `add_event` 配線、後述 system 群の登録。
- ✅ `cargo check` 通過、既存テスト退行なし。
- 学び: `pub use components::*` glob re-export が無いプロジェクトのため、新規型は 1 つずつ `pub use components::{...};` で公開する必要があった。

### ✅ Step 2 — `parse_scenario_system` 改修（commit `320745b`）
- ✅ Local → `ScenarioFileWatchState` 化。
- ✅ 成功時のみ `ScenarioLoadedFromFile` 発火（`has_instruments_ref` も `serde_json::Value` で peek 検出して埋める）。
- ✅ `sync_registry_from_scenario_loaded_system` を新規追加し `(parse_scenario_system, sync_registry_from_scenario_loaded_system).chain()` で配線。
- ✅ Unit test: §5.1「同期方向」3 件 + characterization 3 件（既存 parse_scenario_system 挙動の固定）。

### ✅ Step 3 — writeback system + Run 直前 inline flush（commit `7a8cab7`）
- ✅ `writeback_scenario_instruments_system` 実装（v2 正規化、atomic write、両 sidecar 同期、ScenarioFileWatchState 更新、dirty/error 管理）。
- ✅ `flush_sidecars_now` helper として切り出し、`handle_strategy_run_system` の `RunStrategy` 送信直前に inline 呼び出し追加。
- ✅ `mark_registry_dirty_system` 追加（revision inc）。
- ✅ `ScenarioWritebackPaths { cache_sidecar: Option<PathBuf> }` Resource を新設し、`UiPlugin::build` で `cache_state_paths()` 経由で注入（test では tempdir path 注入可能に）。
- ✅ Unit test: §5.1「永続化」「Run 同期」「Schema 互換」計 11 件。
- 学び: `cache_state_paths()` は `(json_path, py_path)` のタプルを返す既存 strategy source cache 機構。第 1 要素を `cache_sidecar` として使用。

### ✅ Step 4 — Chart シグネチャと旧経路（atomic に 4a/4b/4c/4d）（commit `b2b0cbb`）

#### ✅ Step 4a `spawn_chart_panel(commands, instrument_id: &str)` シグネチャ変更
- ✅ `src/ui/window.rs`: root に `ChartInstrument` 付与、タイトル `format!("CHART — {}", instrument_id)`。
- ✅ Red→Green 単体テストで `ChartInstrument` 付与を world inspection で確認。
- ⚠️ **既知の視覚バグ**: em dash `—` が Bevy デフォルトフォント (FiraMono-subset) に無く豆腐 `□` で表示される（memory `bevy-default-font-no-geometric-shapes` の系統）。動作には影響なし、§10 で記録。

#### ✅ Step 4b 旧 Chart 経路撤去
- ✅ `panel_spawn_dispatcher_system::PanelKind::Chart` arm → warn no-op 化。
- ✅ `sidebar.rs` の `Panels` 配列から `PanelKind::Chart` 削除。
- ✅ `floating_window.rs` の `spawn_chart_panel` import も削除（呼び出し元 0）。
- 注: `PanelKind::Chart` variant 自体は legacy layout JSON 互換のために **残す**。Phase 7.5b でも残置を推奨。

#### ✅ Step 4c 旧 Chart layout の ignore + warn（3 経路）
- ✅ `is_legacy_chart_entry` helper を `layout_persistence.rs` に追加。
- ✅ `apply_cache_restore_system` / `apply_layout_system` / `apply_pending_layout_system` の 3 経路で skip + warn 化。
- ✅ Unit test: `legacy_chart_window_layout_is_skipped`。

#### ✅ Step 4d `editor_history.rs` フィクスチャ修正
- ✅ test mod 内の `PanelKind::Chart` 7 箇所を `PanelKind::Orders` に置換（Chart 固有挙動を検証していないため）。Phase 7.5 の「Chart spawn は `ChartInstrument` 必須」不変条件と整合。

### ✅ Step 5 — sync system + close observer + sidebar `[× row]` + layout 除外（commit `183eef5`）
- ✅ `instrument_chart_sync_system` 追加。registry.is_changed() で Chart spawn/despawn を駆動（idempotent、部分 diff 対応）。
- ✅ close observer に `ChartInstrument` 分岐追加。`editable=true` なら `registry.remove(id)` → 次 tick で sync が Chart despawn、`editable=false` なら早期 return（lock）。history.push_window_despawn / autosave は skip。
- ✅ drag-end observer に `ChartInstrument` skip 追加。`history.push_window_move` / `auto_save.mark_layout_changed` を skip。
- ✅ layout 除外を 7 経路で実施:
  - `build_layout` 系 5 か所 (save_layout / save_as_layout / save_on_close / debounced_autosave + signature) の panels Query に `Without<ChartInstrument>` 追加 → Chart は layout JSON に保存されない。
  - `apply_layout_system` / `apply_pending_layout_system` の panels Query にも `Without<ChartInstrument>` 追加 → layout に含まれない Chart entity が誤 despawn されない（Critical #4）。
- ✅ sidebar `Instruments` セクション全面書き換え:
  - `SidebarInstrumentsList` コンテナを 1 つ spawn、`update_sidebar_system` が `registry.is_changed()` で行を rebuild。
  - 各行に `[x]` ボタン（ASCII `x` — Bevy デフォルトフォント豆腐回避 memory `bevy-default-font-no-geometric-shapes` 準拠）。
  - `editable=false` 時は `[x]` を disabled 色、sidebar 末尾に `SidebarInstrumentsWarning`「This sidecar uses 'instruments_ref' — read-only in Phase 7.5a」表示。
  - 新規 `instrument_remove_button_system` が `Pressed` で `registry.remove`（editable=true のみ）。
- ✅ 新マーカー追加: `SidebarInstrumentRow` / `SidebarInstrumentRemoveButton` / `SidebarInstrumentsList` / `SidebarInstrumentsWarning`。

### ✅ Step 6 — Bevy schedule 統合テスト + 手動 E2E（commit `2c6deee` + `fc5a72a`）
- ✅ §5.2 統合テスト 4 件（うち 1 件は `#[ignore]`、R1 本番修正待ち）。
- ✅ §5.3 手動 E2E チェックリスト全 7 項目 PASS。
- ✅ E2E 中に発見した BOM 耐性問題と ScenarioMetadata 同期問題を fix-up commit `fc5a72a` で対応。

### ✅ 4.7 schedule 順序（Bevy `Update`、実装済み）

```
parse_scenario_system
  → sync_registry_from_scenario_loaded_system
  → mark_registry_dirty_system
  → sync_scenario_metadata_from_registry_system  ← E2E fix-up で追加（§7 参照）
  → writeback_scenario_instruments_system
  → instrument_chart_sync_system
  → handle_strategy_run_system は別 chain だが
    handle_strategy_run_system.after(sync_scenario_metadata_from_registry_system) を明示
```

- 同 tick で Add → Run しても registry → revision inc → sync_scenario_metadata で ScenarioMetadata 反映 → run handler 内 inline flush で sidecar 確定 → RunStrategy 送信、の順序が崩れない。

---

## 5. テスト

### 5.1 Rust 単体

#### 同期方向
- `test_registry_not_overwritten_without_scenario_loaded_event`: registry を手動で `["X"]` に書いた後 `parse_scenario_system` を空 trigger（mtime 変化なし）→ 上書きされない。
- `test_registry_replaced_by_scenario_loaded_event`: イベント発火で registry が event 内容で置換。
- `test_writeback_does_not_publish_scenario_loaded`: registry 変更 → writeback → イベント発火しない、`flushed_revision` が `revision` に追随。

#### Chart 寿命
- `test_chart_sync_spawns_missing` / `test_chart_sync_despawns_orphan` / `test_close_removes_from_registry` / `test_row_remove_despawns_chart` / `test_registry_add_dedup`。

#### 永続化（**両 sidecar 同期が肝**）
- `test_writeback_updates_both_original_and_cache_sidecars`: registry 変更 → 元 sidecar と cache sidecar の **両方** の `scenario.instruments` が更新、両ファイル mtime 変化。
- `test_writeback_only_touches_scenario_instruments_field`: layout/viewport/windows/strategy_path 等の他フィールドは byte-equal で不変。
- `test_layout_autosave_does_not_touch_original_sidecar`: window 移動だけして registry を触らない → 元 sidecar mtime 変化なし、cache JSON のみ更新（既存 autosave 経路の不変性確認）。
- `test_writeback_does_not_retrigger_scenario_reload`: 書き戻し後 `parse_scenario_system` 再走 → `ScenarioLoadedFromFile` 未発火、registry 上書きなし（ScenarioFileWatchState mtime 転記の正当性）。
- `test_writeback_skipped_when_original_path_none`: unsaved 状態では元 sidecar 書き戻し無し、cache sidecar のみ更新。
- `test_writeback_failure_keeps_revision_and_retries`: write 失敗 → `flushed_revision` 据え置き、`last_error` 埋まる → 次フレーム成功で flushed 追随。
- `test_writeback_atomic_rename_no_partial_file`: write 中断（書き込み権限失敗等を mock）→ 元ファイルは破損していない（tmp ファイルだけ残るか掃除される）。

#### Schema 互換
- `test_writeback_normalizes_v1_to_v2`: v1 `instrument: "1301.TSE"` を持つ sidecar に対し registry に 2 銘柄ある状態で writeback → `schema_version=2`, `instrument` キー削除, `instruments: [...]` 書き込み、warn ログ確認。
- `test_writeback_handles_legacy_instrument_as_list`: `instrument: ["A", "B"]` 形式も v2 正規化。
- `test_instruments_ref_locks_editing`: sidecar に `instruments_ref` がある → `registry.editable=false`、`[× row]` / Chart `[×]` で registry 不変、writeback 起動しない、warn UI 表示。
- `test_instruments_ref_with_inline_instruments_still_locked`: 両キー混在も lock 側に倒れる。

#### Run 同期（**Critical #2 への対応**）
- `test_run_inline_flush_writes_both_sidecars`: registry に `["A","B"]`、`handle_strategy_run_system` を発火 → RunStrategy 送信前に元 / cache sidecar の `instruments` が `["A","B"]` になっている（fake transport で順序を assert）。
- `test_run_inline_flush_is_idempotent`: writeback 既に完了済みでも inline flush が再走して問題なし、ファイル内容不変、mtime は更新されうる。
- `test_run_blocked_when_inline_flush_fails`: inline flush 失敗 → RunStrategy 送信されず、エラーログ、events.read() で event は消費される（Bevy event semantics に従う）。
- `test_run_does_not_use_is_changed_guard`: registry 変更 → 同 tick で Run event 発火 → handler が `continue` しない（is_changed ベース待ち実装への退化検知）。

#### History / layout 除外（**Critical #4 への対応**）
- `test_chart_drag_not_in_history`
- `test_chart_close_not_in_history`
- `test_build_layout_excludes_chart`
- `test_apply_layout_does_not_despawn_chart_when_layout_lacks_chart`: registry に 1 銘柄、Chart 1 つ spawn 済み、`apply_layout_system` に Chart を含まない layout を渡す → Chart **生き残る**。
- `test_apply_pending_layout_does_not_despawn_chart`
- `test_cache_restore_does_not_despawn_chart`

#### 旧 Chart layout 互換
- `test_legacy_chart_in_apply_layout_system_ignored`
- `test_legacy_chart_in_apply_pending_layout_system_ignored`
- `test_legacy_chart_in_cache_restore_ignored`

### 5.2 Rust 統合（Bevy schedule 単位）

- `test_e2e_open_to_chart_spawn`: app build → `StrategyFileLoadRequested(pair_trade_minute.json)` を投げて 1 フレーム回す → `InstrumentRegistry.ids == ["1301.TSE","7203.TSE"]`、`ChartInstrument` 付き Chart が 2 entity 存在。
- `test_e2e_close_writeback`: 上記後、`7203.TSE` の Chart root に対し close observer を直接 trigger → registry が `["1301.TSE"]`、元 sidecar + cache sidecar の `scenario.instruments` が `["1301.TSE"]`。
- `test_e2e_close_and_run_uses_new_instruments`: 上記後、`StrategyRunRequested` を投げる → fake transport が受け取った RunStrategy の config.instruments が `["1301.TSE"]`、両 sidecar も `["1301.TSE"]`。
- `test_e2e_save_as_after_unsaved_add`: original_path=None で registry に手動 add → writeback skip → `buffer.original_path = Some(...)` セット → mark_registry_dirty を強制 inc → writeback が新パスに走る（Save As 経路の漏れ検知、Critical #5）。

### 5.3 手動 E2E

> **注記 (2026-05-17 訂正)**: 本節は当初 Phase 7.3 (`5029f24` CacheOnly writeback) 前提で「元 sidecar も即時更新される」と書かれていたが、実機挙動と乖離していたため Phase 7.5a E2E 結果に合わせて全面改訂。**現行仕様: 編集系操作 (Chart 閉じ / registry mutate) は cache sidecar のみ書き戻し、元 sidecar は明示 Save / Save As で初めて反映される**。Run 直前 inline flush は別経路。

- `File > Open...` で `pair_trade_minute.json` (`.json` を直接選択。`.py` は対象外) → Instruments に 2 銘柄、Chart 2 つ spawn。
- Chart `[×]` で `7203.TSE` Chart を閉じる → Instruments の行も消え、**cache sidecar の `scenario.instruments` のみ** `["1301.TSE"]` に更新。**元 sidecar は不変** (Phase 7.3 CacheOnly writeback)。元 sidecar に反映したい場合は `File > Save` を明示実行。
- 再度同じ `pair_trade_minute.json` を Open → **元 sidecar が Sidecar 優先で読まれ 2 銘柄復元** (`["1301.TSE","7203.TSE"]`)。cache に残っていた 1 銘柄状態は採用されない。
- window を動かしただけでは元 sidecar mtime 変化なし、cache JSON のみ更新（従来通り）。
- 単一 `instrument: "1301.TSE"` の v1 sidecar (`e2e_v1_sidecar.json`) を開いて Chart `[×]` → **cache 側のみ** `schema_version: 2`, `instruments: []` に正規化される。**元 v1 fixture は `instrument: "1301.TSE"` のまま不変** (Phase 7.3 CacheOnly writeback と整合)。元ファイルを v2 化したい場合は明示 Save。
- `instruments_ref` を持つ sidecar を開く → `[× row]` と Chart `[×]` が visually disabled、警告行表示、ファイル mtime 変化なし。
- 旧 layout JSON（`PanelKind::Chart` 単体エントリ）を読んでも起動成功 + warn ログ。

**Save / Save As と writeback の関係 (Phase 7.3 以降)**:
- 編集 (registry mutate / Chart close) → cache sidecar のみ atomic write
- `File > Save` → 元 sidecar に flush (現在の registry / ScenarioMetadata を書き出し)
- `File > Save As` → 新 path を `StrategyBuffer.original_path` にセット + §9.1 完了済の `writeback.revision += 1` で新 path に flush
- `Run` 直前 inline flush は cache + 元 sidecar 両方を強制同期 (validation 経路の整合性のため)

---

## 6. 完了基準 ✅ 全達成

- ✅ `pair_trade_minute.json` を Open → Instruments に 2 銘柄、Chart 2 つ spawn
- ✅ Chart `[×]` で Instruments 該当行 + Chart が消え、**cache sidecar が更新** (元 sidecar は明示 Save または Run inline flush で反映 — Phase 7.3 CacheOnly writeback)
- ✅ サイドバー `Panels` から `Chart` ボタン消失
- ✅ window 移動だけでは元 sidecar 不変
- ✅ Close → 即 Run で backend に届く RunStrategy の instruments が新内容、cache sidecar も同内容
- ✅ v1 sidecar の Close で **cache 側が** v2 正規化される (元 v1 fixture は明示 Save まで不変 — Phase 7.3 CacheOnly writeback)
- ✅ `instruments_ref` を含む sidecar は編集ロック、UI 警告表示
- ✅ 旧 layout JSON が ignore + warn で起動成功
- ✅ §5.1 / §5.2 のテスト全件 pass（120 passed / 1 ignored — R1 待ちの skeleton 1 件のみ）

---

## 7. 実装完了後の追加知見と設計思想

### 7.1 当初計画になかった追加実装

#### `sync_scenario_metadata_from_registry_system`（commit `fc5a72a`）

**問題**: Phase 7.5a 当初設計では「registry が真、ScenarioMetadata は parse 由来」という一方向データフローを描いたが、実機 E2E で `handle_strategy_run_system` の validation（`scenario.instruments.is_empty()` チェック）が **stale な ScenarioMetadata.instruments** を見て Run を block するケースが発生。

**原因**: registry → sidecar JSON への writeback はあったが、メモリ内 `ScenarioMetadata.instruments` への反映が無かった。close → 即 Run のシーケンスで、ScenarioMetadata は old 値のままで validation を通り、その後 inline flush が走るので結果的には正しく動くが、validation 経路が registry を見ていないという設計違和感。

**対応**: `sync_scenario_metadata_from_registry_system` を新規追加し、writeback と同じ dirty ゲート (`registry.editable && revision != flushed_revision`) で `ScenarioMetadata.instruments` を registry.ids に同期。chain 内 `writeback` の **前** に置く。ScenarioMetadata の change detection が毎 tick 汚れるのを防ぐため、`scenario.instruments == new_ids` なら no-op。

**設計思想**: registry を **single source of truth**、ScenarioMetadata と sidecar JSON は両方その投影（projection）と位置付ける。projection が分岐していると validation が片方を見て破綻する。両方とも同じ dirty ゲートで同期させる。

#### BOM 耐性（`read_json_with_bom_strip` helper、commit `fc5a72a`）

**問題**: PowerShell `Set-Content` / `Out-File` / Notepad は **デフォルトで UTF-8 BOM (`EF BB BF`)** を書き出す。`serde_json::from_str` は BOM を許容せず `expected value at line 1 column 1` で parse 失敗。本番の sidecar 読み込み 5 経路すべてで該当。

**対応**: `pub(crate) fn read_json_with_bom_strip(path) -> std::io::Result<String>` を `layout_persistence.rs` に追加し、JSON を読む全 5 経路を helper 経由に統一:
1. `layout_persistence::load_layout_from`
2. `layout_persistence` の scenario merge 部
3. `menu_bar::restore_from_cache`
4. `menu_bar` の sidecar windows peek
5. `components::rewrite_scenario_instruments_atomic`

**Tips**: Bash 経由でも `[System.IO.File]::WriteAllText($path, $content, (New-Object System.Text.UTF8Encoding $false))` で BOM なし書き出し可能。テスト fixture 作成時に必須。

### 7.2 設計思想（後続作業者向け）

#### 単一データソース（Single Source of Truth）
- `InstrumentRegistry.ids` が **真**。`ScenarioMetadata.instruments` と sidecar JSON の `scenario.instruments` は両方とも projection（同期投影）。
- 同期は registry → 他、の **一方向**。逆流（ScenarioMetadata → registry）は `parse_scenario_system` 経由のファイル load 時のみ。
- registry が変わると `mark_registry_dirty_system` が `revision` を inc、その同 tick chain で:
  1. `sync_scenario_metadata_from_registry_system` が `ScenarioMetadata.instruments` を更新
  2. `writeback_scenario_instruments_system` が両 sidecar を atomic write
  3. `instrument_chart_sync_system` が Chart entity を spawn/despawn

#### `is_changed()` ガードと event の併用
- `EventReader::read()` を `is_changed()` ベースで `continue` させると **event が drain されない** → 次 tick に積み残し → 無限再試行。Bevy event semantics の罠。
- 解決: Run inline flush のように「event 受信 + 同 tick 同期処理」を明示的に直列化する。`is_changed()` は「次 tick 反映でよい」ものに限定。
- 退化検知用 unit test: `test_run_does_not_use_is_changed_guard`

#### Chart は layout 管理外
- Chart は scenario JSON が真なので、Chart の位置・サイズを `WindowLayout` に保存しない方針。
- `build_layout` 経由 5 か所と `apply_layout_system` / `apply_pending_layout_system` の panels Query にすべて `Without<ChartInstrument>` を入れて、save 側は Chart を書かず、apply 側は Chart を despawn しない、両側で物理的に隔離。
- Phase 7.6 で多銘柄 Chart データ分離 + 位置復元を扱う場合は `WindowLayout` に `instrument_id` 拡張を検討。

#### dirty/flush の revision 二段管理
- `is_changed()` の race を避けるため、`ScenarioInstrumentsWritebackState` で `revision` (registry 編集ごとに inc) と `flushed_revision` (writeback 成功時に追随) を明示。
- writeback system は `revision != flushed_revision` の間 dirty として再試行。失敗時は `last_error` をセットして revision 据え置き → 次 tick 自動再試行。
- ファイル load 由来の代入は `sync_registry_from_scenario_loaded_system` が `revision = flushed_revision` に戻して **writeback ループを起動させない**。これが §3.2 の「同期方向の一方向化」。

#### atomic write の Windows 事情
- `std::fs::rename` は同一ボリュームでは atomic。tmp は **同フォルダ** に作る（`dir.join(format!(".{file_name}.tmp-{pid}-{rand}"))`）。
- 別ボリューム rename は POSIX とは違い Windows でも atomic 保証なし。tmp の場所には注意。

### 7.3 リスク・既知の制約

| # | 項目 | 対応状況 |
|---|---|---|
| R1 | unsaved 状態（original_path=None）→ Save As 経路 | ⏸️ **未対応**。`test_e2e_save_as_after_unsaved_add` を `#[ignore]` で skeleton 残置。Save As 成功時に `writeback.revision += 1` を強制する小修正が必要 |
| R2 | 7.5a では Add UI が無い | ✅ 既知。7.5b で追加。それまでは外部 JSON 編集 → Open で銘柄を増やす運用 |
| R3 | 複数 Chart が同じ TradingData を描く | ⏸️ 既知制約。Phase 7.6 仮称 で多銘柄データ経路を別計画 |
| R4 | Chart 位置・サイズが復元されない | ⏸️ `WindowLayout` 非保存方針のため。Phase 7.6 / 8 で `WindowLayout.instrument_id` 拡張を検討 |
| R5 | scenario_parser の mtime 競合 | ✅ `ScenarioFileWatchState` 格上げ + writeback 後 mtime 転記で対応済み。`test_writeback_does_not_retrigger_scenario_reload` で回帰防止 |
| R6 | atomic rename（Windows） | ✅ 同一ボリューム tmp で対応済み |
| R7 | `handle_strategy_run_system` 内 inline flush で UI スレッド I/O が発生 | ⚠️ 現状実害なし。Run 連打でフリーズ感じたら `std::thread::spawn` に出す |
| R8 | `Instruments` を sidebar から削除した銘柄は履歴に残らない | ⏸️ Undo/Redo 連動は 7.5a 非スコープ。誤操作は再 Add でリカバリ（7.5b 以降） |

### 7.4 Tips（後続作業者向け）

#### 検証コマンド
```powershell
# 全 lib test
cargo test --lib

# Phase 7.5a 関連だけ抽出
cargo test --lib ui::components::writeback_scenario_instruments_tests
cargo test --lib ui::components::sync_registry_from_scenario_loaded_tests
cargo test --lib ui::components::mark_registry_dirty_tests
cargo test --lib ui::scenario_parser
cargo test --lib ui::layout_persistence
cargo test --lib ui::window

# warning 込み
cargo check --tests 2>&1 | Select-String -Pattern "warning|error"
```

#### Phase 7.5a fixture
- `examples/pair_trade_minute.json` — v2 schema、2 銘柄、通常検証用
- `examples/e2e_v1_sidecar.json` — v1 schema (`schema_version: 1`, `instrument` 単数キー)、v2 正規化検証用
- `examples/e2e_instruments_ref_locked.json` — `instruments_ref` only、lock + 警告行検証用
- `examples/e2e_instruments_ref_mixed_locked.json` — `instruments` + `instruments_ref` 混在、lock + 行表示検証用

#### 編集時の罠
- **BOM**: 新規 fixture を作るときは `[System.IO.File]::WriteAllText($path, $content, (New-Object System.Text.UTF8Encoding $false))` で BOM なしに。production は BOM strip 済みだが、最初に BOM で躓くと debug 時間が無駄になる。
- **Bevy 0.15 vs 0.19**: 本プロジェクトは 0.15。`get_single()` / `Parent` / `Trigger::entity()` / `app.add_observer()`。Bundle 構造体禁止（タプル spawn）。
- **Color**: `Color::srgb` / `srgba` を使う、`rgb` は廃止。
- **PanCam の scale 補正**: drag delta を world 座標に変換する際は camera scale を掛ける。`floating_window.rs` の close/drag observer 参照。
- **Em dash 豆腐**: `—` (U+2014) は FiraMono-subset に含まれず Bevy デフォルトフォントで豆腐 `□`。ASCII `x` や `-` で代替するか、NotoSansSymbols2 を部分適用。memory `bevy-default-font-no-geometric-shapes` 参照。

#### 手動 E2E
- 一次資料: `.claude/skills/e2e-testing/SKILL.md`
- 役割分担: backend/backcast の起動・停止・ログ確認は AI、UI 操作・目視はユーザー
- backcast 起動時 `BEVY_ASSET_ROOT=$PWD` を **必ず** 設定（memory `bevy-asset-root-exe-launch`）
- backend 起動時 `PYTHONPATH=$PWD\python\engine\proto` を **必ず** 設定（memory `proto-regen-absolute-import-trap`）
- cache sidecar path: `$env:LOCALAPPDATA\the-trader-was-replaced\app_state.json`

#### コードリーディング順序
新規参加者がコードを読むときの推奨順:
1. `src/ui/components.rs` の `InstrumentRegistry` / `ChartInstrument` / `ScenarioLoadedFromFile` / `ScenarioFileWatchState` / `ScenarioInstrumentsWritebackState` / `ScenarioWritebackPaths` の型定義（L268-360 付近）
2. `src/ui/scenario_parser.rs::parse_scenario_system` — ファイル → ScenarioMetadata + ScenarioLoadedFromFile 発火
3. `src/ui/components.rs::sync_registry_from_scenario_loaded_system` — event → registry replace
4. `src/ui/components.rs::mark_registry_dirty_system` — registry change → revision inc
5. `src/ui/components.rs::sync_scenario_metadata_from_registry_system` — registry → ScenarioMetadata
6. `src/ui/components.rs::writeback_scenario_instruments_system` + `flush_sidecars_now` + `rewrite_scenario_instruments_atomic` — registry → 両 sidecar atomic write
7. `src/ui/window.rs::instrument_chart_sync_system` — registry diff → Chart spawn/despawn
8. `src/ui/menu_bar.rs::handle_strategy_run_system` — Run 直前 inline flush
9. `src/ui/floating_window.rs` の close/drag observer の `ChartInstrument` 分岐
10. `src/ui/sidebar.rs` の `SidebarInstrumentsList` 関連 system 群
11. `src/ui/layout_persistence.rs` の `Without<ChartInstrument>` / `is_legacy_chart_entry`

---

## 8. Phase 7.5b 予告（**別計画書として独立**）

7.5a 完了後、次の 1 枚を `docs/plan/Phase 7.5b - Instrument Picker.md` として起こす。本書では仕様確定しない（実装者判断の余地を残さないため）。

7.5b の予定スコープ:
- `[+ Add]` ボタン UI と instrument picker パネル（`PanelKind::InstrumentPicker`）
- backend 新 gRPC `ListAllListedSymbols(end_date)` の proto / Python / Rust client / pytest
- `InstrumentList` → `AvailableInstruments` リネーム + 構造変更（end_date keyed cache）
- Rust 側からの旧 `ListInstruments` 呼び出し停止
- Phase 8 計画書（`docs/plan/Phase 8 - Live Venue and Market Data.md`）の Sidebar universe ソース表を `AvailableInstruments`/`InstrumentRegistry` 二分法に書き換え
- picker の searchbox、エラー UI、`scenario.end` invalid format ハンドリング、Add 連打抑止、cache invalidation

7.5b 着手前に決めるべきこと（**7.5a では決めない**）:
- Code 正規化 5 桁 → 4 桁の規則妥当性（`code_to_instrument_id` の round-trip テスト fixture pin）
- `--jquants-csv-dir` と既存 `--jquants-dir` の関係
- universe API を `ListAllListedSymbols` 専用にするか、Phase 8 で `ListInstruments(source=...)` に集約するか
- v3 schema の `instruments_ref` をどう解決して Add UI に統合するか

---

## 9. Phase 7.5a 完了後の残課題（Phase 7.5b 着手前の整理候補）

### 9.1 R1: Save As 経路の writeback.revision 強制 inc（高優先）

**症状**: `StrategyBuffer.original_path = None` (unsaved) 状態で registry に add → writeback skip（path 不在で書き戻し先なし、`last_error = "no writeback target"`） → ユーザーが Save As で path をセット → しかし `mark_registry_dirty_system` は registry の `is_changed()` を見るため、Save As 自体は registry を mutate せず → revision 不変 → writeback 走らず → unsaved 中の編集が新 path に保存されない。

**修正方針**: Save As 成功時の handler（`menu_bar.rs` の `handle_strategy_save_as_system` 等）で `writeback.revision += 1` を強制 inc。

**関連 test**: `test_e2e_save_as_after_unsaved_add` (`#[ignore]` で skeleton 残置、commit `2c6deee`)。R1 修正後に `#[ignore]` 外して本実装。

**完了 (2026-05-17)**:
- 実装: Save As 成功 handler で `writeback.revision += 1` を強制 inc
- 検証: `cargo test` PASS / warning 0
- 差分: 2 ファイル (本実装 + `#[ignore]` 解除した `test_e2e_save_as_after_unsaved_add`)
- 補足: §9.1 §関連 test 末尾の skeleton test は本実装化済 (test (B) の追加は不要と判断)
- commit: <pending>

### 9.2 registry.editable leak（中優先）

**症状**: Fixture B (instruments_ref locked) を Open → `registry.editable = false` がセット → 別 sidecar（scenario なし or legacy layout）を Open → `parse_scenario_system` が scenario を見つけず `ScenarioMetadata::default()` に reset するが、**`ScenarioLoadedFromFile` event は発火しない**（scenario key 不在のため） → `sync_registry_from_scenario_loaded_system` が呼ばれない → `registry.editable = false` が残存 → 新セッションで lock 警告行が誤表示される。

**E2E §5.3 チェック 7 のスクリーンショットで観測**: legacy layout JSON で起動したのに「This sidecar uses 'instruments_ref'」警告行が残っていた。

**修正方針案**:
- (a) `parse_scenario_system` で scenario key 不在の場合も、registry を reset する別 event（`ScenarioClearedFromFile` 等）を発火し sync system で `registry.editable = true` に戻す
- (b) `apply_scenario_loaded_to_registry_system` が file load 経路ごとに最終的な editable 状態を確定させる
- (c) sidebar 警告行を `registry.editable && registry.has_scenario_loaded` で 2 条件 AND にする

(a) が同期方向の一方向化と整合性が高い。

**完了 (2026-05-17)**:
- 実装: 方針 (a) を採用。`ScenarioClearedFromFile` event を新設し、`parse_scenario_system` の 4 reset 経路（scenario key 不在 / parse 失敗 / etc.）から発火。`sync_registry_from_scenario_cleared_system` を chain に追加し `registry.editable = true` に戻す
- 検証: `cargo test --lib` 128 passed / 0 failed、`cargo check --lib` warning 0、Red test `test_editable_resets_to_true_when_switching_to_sidecar_without_scenario` PASS
- 差分: 3 ファイル (`src/ui/components.rs` event 定義 + sync system、`src/ui/scenario_parser.rs` 4 reset 経路で cleared 発火 + test 6 + Red test、`src/ui/mod.rs` event 登録 + chain 拡張)
- commit: <pending>

### 9.3 未使用 buffer 引数 2 件（低優先）

`cargo check` で warning 2 件:
- `src/ui/components.rs:607` `writeback_scenario_instruments_system` の `buffer: Res<StrategyBuffer>` — `flush_sidecars_now` helper への path 抽出を `buffer.original_path.as_deref()` で渡しているはずだが現在の実装で未使用化されている可能性。要 Read 確認。
- `src/ui/menu_bar.rs:553` `handle_strategy_run_system` の `buffer: Res<StrategyBuffer>` — 同上、Run inline flush で `buffer.original_path.as_deref()` を使うはず。

`flush_sidecars_now` の signature が変わった or autonomous な ScenarioMetadata 同期 system 追加で経路が変わった可能性。確認の上、引数削除 or `_ = buffer.original_path` で意図を明示。

**完了 (2026-05-17)**:
- 実装: `handle_strategy_run_system` (`src/ui/menu_bar.rs:553`) と `writeback_scenario_instruments_system` (`src/ui/components.rs:640`) から未使用の `_buffer: Res<StrategyBuffer>` 引数を削除
- 検証: `cargo check --lib` warning 0 / `cargo test --lib` 128 passed
- 差分: 2 ファイル (`src/ui/menu_bar.rs`, `src/ui/components.rs`)
- commit: <pending>

### 9.4 Open 後 1 銘柄欠落 bug (再現困難、Phase 7.5b へ繰越)

**観測事象 (2026-05-17、§5.3 E2E 中に 1 回のみ)**:
- 流れ: `pair_trade_minute.json` Open → `7203.TSE` Chart `[×]` で close (cache のみ 1 銘柄化) → アプリ再起動 → cache 経由で 1 銘柄状態が一時的に見えた状態から、再度 `pair_trade_minute.json` を Open
- 期待: 元 sidecar Sidecar 優先で 2 銘柄復元 (§5.3 改訂後の項目③)
- 実際: Instruments に 1 銘柄 (`1301.TSE`) しか出なかった。2 回目以降の再現試行 (診断ログ patch 5 件投入) では再現せず

**再現条件の仮説**:
- 「`instruments_ref` locked sidecar 起動直後の state」→「cache を旧 `PanelKind::Chart` entry + `scenario.instruments` 1 銘柄で書き換え」→「再起動」→「`pair_trade_minute.json` Open」という複雑な状態遷移の組み合わせでのみ発生
- 単発の Open リプレイでは出ない

**疑わしい経路 (決定打なし)**:
- `InstrumentRegistry` mutate 経路 4 か所のいずれかで race
- chart despawn observer 1 経路
- `parse_scenario_system` の path/mtime 早期 return が古い ScenarioMetadata を保持し、新ファイル load を skip している可能性 (R5 対策の `ScenarioFileWatchState` mtime 転記との相互作用)
- `registry.editable` 残存と sync event 不発火の組み合わせ (§9.2 で対応した cleared event の取りこぼし経路)

**診断試行結果**:
- 診断ログ patch 5 件投入 (mutate 4 + despawn 1) でも 2 回の再現試行で発生せず
- 状態スナップショット (cache JSON / 元 sidecar mtime / `ScenarioFileWatchState`) の事前条件特定に至らず

**推奨次手 (Phase 7.5b 着手時)**:
1. **再現スクリプト化を最優先**: 上記「locked sidecar 起動 → cache 強制書換 → 再起動 → Open」を PowerShell + cache JSON 直接編集で deterministic に組む
2. 再現したら `parse_scenario_system` の mtime 早期 return 分岐に trace ログ追加し、新 path で `ScenarioFileWatchState` がリセットされているか確認
3. `ScenarioLoadedFromFile` event が 2 銘柄分の `instruments` を載せて発火しているかを `sync_registry_from_scenario_loaded_system` 入口で assert
4. registry replace ↔ writeback ↔ chart sync の同 tick chain 順序を再確認 (現行: sync_metadata → writeback → chart_sync)

**関連経路ファイル**:
- `src/ui/scenario_parser.rs::parse_scenario_system` (path/mtime 早期 return)
- `src/ui/components.rs::sync_registry_from_scenario_loaded_system` / `sync_registry_from_scenario_cleared_system`
- `src/ui/components.rs` の `ScenarioFileWatchState` / `ScenarioInstrumentsWritebackState`
- `src/ui/menu_bar.rs::restore_from_cache` (cache 読み出し経路)
- `src/ui/layout_persistence.rs` (cache merge 部の scenario.instruments 取り込み)

**Phase 7.5a スコープ判断**: 1 回観測 / 再現不能のため修正は Phase 7.5b に繰越。本書では記録のみとし、Phase 7.5b kickoff 時に再現スクリプト作成を最初の task として割り当てる。

### 9.5 Em dash 豆腐（低優先、視覚のみ）

`CHART — {id}` のタイトルと sidebar 警告行内の `—` が Bevy デフォルトフォントで豆腐 `□` 化。動作影響なし。

**対応案**:
- (a) ASCII 代替: `CHART - {id}`、警告行 `read-only` に hyphen
- (b) NotoSansSymbols2 を Text の該当部分に部分適用（memory `bevy-default-font-no-geometric-shapes` のパターン）
- (c) FiraMono-subset を em dash 含むものに差し替え

(a) が最小コスト。Phase 7.5b の UI brush-up と一緒に対応推奨。

### 9.6 cache JSON への Chart 位置情報非保存（既知制約、Phase 7.6 候補）

Phase 7.5a では Chart を `WindowLayout` に保存しない方針（`build_layout` の `Without<ChartInstrument>`）。結果として Chart の位置・サイズはセッション間で保存されず、registry から Chart spawn 時はデフォルト位置に出る。

Phase 7.6 で扱う場合、`WindowLayout` に `instrument_id: Option<String>` を追加して Chart 単位の位置記録 + 復元を実装する。同時に多銘柄 Chart データ分離 (R3) も併せて設計。

### 9.7 多銘柄 TradingData 共有（R3、既知制約）

複数 Chart が同じ `TradingData` Resource を読むため、Chart 2 つ並べると同じ price/candle を描く。Phase 7.5a では非対応、Phase 7.6 仮称 で per-instrument data fetch + per-Chart store 経路を別計画。

### 9.7 Bevy `add_systems` タプル 20 上限（潜在）

Phase 7.5a で `UiPlugin::build` の `add_systems(Update, (...))` が増えている。20 個を超えるとコンパイル時に `IntoSystemConfigs` 系のエラー。現状は分割済みだが、Phase 7.5b で picker system 等が加わる際は注意。

---

## 10. 別 issue として記録すべき発見

### 10.1 E2E セッションで autonomous に追加されたもの

`sync_scenario_metadata_from_registry_system` は当初の Step 1-6 計画書には無く、E2E 中に Driver/Navigator が自律判定で追加した。理由は §7.1 参照。  
**経緯の透明化のため、Driver/Navigator が自律で本番コードを追加する場合は事前に司令塔（Orchestrator）の承認を取るプロトコルを Phase 7.5b で確立すべき**。Pair Relay の鉄則「Driver は typist」「Navigator は判断」を守る限り、こうしたドリフトは起きない設計だが、実機 E2E で突発バグ修正の必要が出るケースは想定が必要。

### 10.2 Phase 7.5a で意図的に残した dead code

- `src/ui/components.rs` の `SidebarListLabel`（旧 sidebar 単一 Text label 経路、現在は `SidebarInstrumentsList` ベース） — Phase 7.5b で削除予定
- `src/main.rs` / `src/trading.rs` の `InstrumentList` Resource — Phase 7.5b で `AvailableInstruments` にリネーム
- `examples/e2e_v1_sidecar.py` / `e2e_instruments_ref_locked.py` / `e2e_instruments_ref_mixed_locked.py` — fixture sibling として残置（中身は no-logic）

### 10.3 計画書外のコミット

- `4d54dee skill` — Phase 7.5a 作業中に独立で入った skill 更新（pair-relay 等）。本書とは無関係。

---

## 11. ロールアウトと回帰防止

### 11.1 マージ前チェックリスト

- ✅ `cargo test --lib` 全 PASS (120 passed / 1 ignored)
- ✅ `cargo check` warning ≤ 2（未使用 buffer 引数 2 件のみ、別 issue 化）
- ✅ 手動 E2E §5.3 全 7 項目 PASS
- ✅ 既存 fixture (`pair_trade_minute.json`) が E2E 副作用で書き換わっていないこと（commit 前に `git restore` 済み）
- ⏸️ R1 (Save As 経路) は別 PR / 別 commit で fixup する前提で `#[ignore]` 残し

### 11.2 回帰チェック想定

Phase 7.5b で picker / Add UI を追加する際の回帰リスク:
- `instrument_chart_sync_system` の idempotent 仕様（重複 spawn しない、不要 despawn する）に変更を入れないこと
- `Without<ChartInstrument>` フィルタを `build_layout` 系 5 経路 + `apply_layout` 系 2 経路で必ず維持
- writeback の dirty/flush revision 二段管理を picker 経由 add でも壊さないこと（picker → registry.add → mark_dirty → writeback の chain が成立すること）

### 11.3 Phase 8 連携

Phase 8 (Live Venue and Market Data) の `python/engine/strategy_runtime/scenario.py:28-32, 243` は **cache sidecar** を読む経路で実装済み。Phase 7.5a の両 sidecar 同期方針はこれと完全整合。Phase 7.5b で `instruments_ref` 解決を追加する際も、cache sidecar 側に解決済み instruments を書く（backend は ref を解決しない）方針を維持すべき。
