# Phase 7.5 — Instruments パネル再設計（Scenario 駆動 + Chart 寿命連動）

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

### Step 1 — 型と Resource、配線
- `src/ui/components.rs`: `InstrumentRegistry`(+helper), `ChartInstrument`, `ScenarioLoadedFromFile`, `ScenarioFileWatchState`, `ScenarioInstrumentsWritebackState` を追加。
- `src/ui/mod.rs`: `init_resource` / `add_event` 配線、後述 system 群の登録（順序は §4.7）。
- このコミットで `cargo check` 通過、既存テスト退行なし。

### Step 2 — `parse_scenario_system` 改修
- Local → `ScenarioFileWatchState` 化。
- 成功時のみ `ScenarioLoadedFromFile` 発火（`has_instruments_ref` も埋める）。
- `sync_registry_from_scenario_loaded_system` を新規追加。
- Unit test: §5.1「同期方向」。

### Step 3 — writeback system + Run 直前 inline flush
- `writeback_scenario_instruments_system` を実装（v2 正規化、atomic write、両 sidecar 同期、ScenarioFileWatchState 更新、dirty/error 管理）。
- `flush_sidecars_now` helper を切り出し `handle_strategy_run_system` に inline 呼び出し追加。
- `mark_registry_dirty_system` 追加（revision inc）。
- Unit test: §5.1「永続化」「Run 同期」。

### Step 4 — Chart シグネチャと旧経路（atomic に 4a/4b/4c/4d）

#### Step 4a `spawn_chart_panel(commands, instrument_id: &str)` シグネチャ変更
- `src/ui/window.rs`: root に `ChartInstrument` 付与、タイトル `"CHART — {id}"`。
- 既存呼び出しは一時的に `"dummy"` で通してビルド維持。

#### Step 4b 旧 Chart 経路撤去
- `panel_spawn_dispatcher_system::PanelKind::Chart` arm → warn no-op。
- `sidebar.rs:61-68` の `Panels` 配列から `PanelKind::Chart` 削除。
- Step 4a で `"dummy"` にした dispatcher 呼び出しを削除。

#### Step 4c 旧 Chart layout の ignore + warn（3 経路）
- `apply_layout_system` / `apply_pending_layout_system` / cache restore の各経路で skip + warn。
- Unit test: §5.1「旧 Chart layout 互換」3 件。

#### Step 4d `editor_history.rs` フィクスチャ修正
- `PanelKind::Chart` 直接 spawn を別 PanelKind に差し替え or helper 化。
- 既存テスト全件 pass。

### Step 5 — sync system + close observer + sidebar `[× row]` + layout 除外
- `instrument_chart_sync_system` 追加（§3.6）。
- close observer に `ChartInstrument` 分岐追加（§3.7）。
- sidebar `Instruments` セクションを registry ベースに書き換え（§3.11、`[× row]` クリック observer 含む）。
- drag-end / `build_layout` / `apply_layout_system::to_despawn` の 3 か所で `ChartInstrument` 除外（§3.9）。
- Unit test: §5.1「Chart 寿命」「History / layout 除外」。

### Step 6 — Bevy schedule 統合テスト + 手動 E2E
- §5.2 統合テスト 3 件。
- §5.3 手動 E2E チェックリスト消化。
- Phase 7.5a 完了 → Phase 7.5b 計画書着手。

### 4.7 schedule 順序（Bevy `Update`）

```
parse_scenario_system
  → sync_registry_from_scenario_loaded_system
  → mark_registry_dirty_system
  → writeback_scenario_instruments_system
  → instrument_chart_sync_system
  → (Run 経路: handle_strategy_run_system は別 chain)
```

- `handle_strategy_run_system` は `mark_registry_dirty_system.before(handle_strategy_run_system)` を明示。同 tick で Add → Run しても registry → revision inc → run handler 内 inline flush で sidecar 確定 → RunStrategy 送信、の順序が崩れない。

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

- `pair_trade_minute.json` Open → Instruments に 2 銘柄、Chart 2 つ spawn。
- Chart `[×]` で `7203.TSE` Chart を閉じる → Instruments の行も消え、元 sidecar と cache sidecar の `scenario.instruments` が `["1301.TSE"]`。
- 再度 Open → 1 銘柄だけ復元。
- window を動かしただけでは元 sidecar mtime 変化なし、cache JSON のみ更新。
- 単一 `instrument: "1301.TSE"` の v1 sidecar を開いて Chart `[×]` → 元 sidecar が `schema_version=2`, `instruments: []` に正規化されている。
- `instruments_ref` を持つ sidecar を開く → `[× row]` と Chart `[×]` が visually disabled、警告行表示、ファイル mtime 変化なし。
- 旧 layout JSON（`PanelKind::Chart` 単体エントリ）を読んでも起動成功 + warn ログ。

---

## 6. 完了基準

- [ ] `pair_trade_minute.json` を Open → Instruments に 2 銘柄、Chart 2 つ spawn
- [ ] Chart `[×]` で Instruments 該当行 + Chart が消え、**元 + cache** の両 sidecar が更新
- [ ] サイドバー `Panels` から `Chart` ボタン消失
- [ ] window 移動だけでは元 sidecar 不変
- [ ] Close → 即 Run で backend に届く RunStrategy の instruments が新内容、cache sidecar も同内容
- [ ] v1 sidecar の Close が v2 正規化される
- [ ] `instruments_ref` を含む sidecar は編集ロック、UI 警告表示
- [ ] 旧 layout JSON が ignore + warn で起動成功
- [ ] §5.1 / §5.2 のテスト全件 pass

---

## 7. リスク・既知の制約

| # | 項目 | 対応 |
|---|---|---|
| R1 | unsaved 状態（original_path=None）→ Save As 経路 | Save As 成功時に `mark_registry_dirty_system` を強制 inc させる小修正を Step 3 に含める。テスト `test_e2e_save_as_after_unsaved_add` |
| R2 | 7.5a では Add UI が無い | 7.5b で追加。それまでは外部 JSON 編集 → Open で銘柄を増やす運用 |
| R3 | 複数 Chart が同じ TradingData を描く | 既知の制約として 0.3 に明示。Phase 7.6 仮称 で多銘柄データ経路を別計画 |
| R4 | Chart 位置・サイズが復元されない | `WindowLayout` 非保存方針のため。Phase 7.6 / 8 で `WindowLayout.instrument_id` 拡張を検討 |
| R5 | scenario_parser の mtime 競合 | `ScenarioFileWatchState` 格上げ + writeback 後 mtime 転記。`test_writeback_does_not_retrigger_scenario_reload` で回帰防止 |
| R6 | atomic rename（Windows） | `std::fs::rename` は同一ボリュームでは atomic。tmp は同フォルダに作る。`tempfile::NamedTempFile::persist` を使うのも可 |
| R7 | `handle_strategy_run_system` 内 inline flush で UI スレッド I/O が発生 | 1 ファイル数 KB の write は実害なし。問題化したら std::thread に出す |
| R8 | `Instruments` を sidebar から削除した銘柄は履歴に残らない | Undo/Redo 連動は 7.5a 非スコープ。誤操作は再 Add でリカバリ（7.5b 以降） |

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
