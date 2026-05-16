# Strategy Editor: multi-spawn + region marker merge/split

## 実装進捗ログ

### 2026-05-16 pair-relay 開始

**作業ブランチ**: `feature/7.3-Scenario-Sidecar-Migration`

**全体ステップ計画**:
```
✅ Step 1:  components.rs — 新型追加 + StrategyBuffer 更新 + 旧型削除
✅ Step 1 再実施 2026-05-16: cargo check 後 24件の意図的 migration error (compile-driven migration パターン通り)
✅ Step 2:  editor_history.rs + WindowLayout — region_key 対応型更新 (2026-05-16)
✅ Step 3:  strategy_editor.rs — merge/split 純粋関数 + ユニットテスト (2026-05-16, cargo check 24件 baseline 変化なし)
✅ Step 4:  floating_window.rs — spawn API を StrategyEditorSpawnSpec 対応 (2026-05-16, cargo check 24→19)
✅ Step 5:  strategy_editor.rs — per-region sync systems 書き換え (2026-05-16, cargo check 19→12, 2 sub-step)
✅ Step 6:  sidebar.rs — blank-spawn に変更 (2026-05-16, cargo check 12→12 内訳変化のみ)
✅ Step 7:  menu_bar.rs — 新メニュー + handle_strategy_file_load_system (2026-05-16, cargo check 12→7, 3 sub-step)
✅ Step 8:  layout_persistence.rs — region-aware lookup + 旧 sidecar 監視経路削除 (2026-05-16, cargo check 7→6)
✅ Step 9:  footer.rs + strategy_editor.rs — Run フローを fragments + 新 flush に追従 (2026-05-16, cargo check 6→4)
✅ Step 10: mod.rs system ordering + 全 sweep cleanup (2026-05-16, cargo check 4→0 🎉)
```

**🎉 lib 本体 cargo check クリーン達成 (2026-05-16): ベースライン 24 → 0 errors**

**Step 1 完了 (components.rs)**:
- `OpenStrategyRequested` + `PendingStrategyLoad` 削除
- `StrategyBuffer`: `source`/`dirty` 削除 → `last_merged_source: Option<String>` 追加
- `PanelSpawnRequested` に `strategy_spec: Option<StrategyEditorSpawnSpec>` 追加
- 追加型: `StrategyEditorId`, `StrategyFragment`, `RegionKeyAllocator`, `PendingStrategyFragments`, `StrategyEditorSpawnSpec`, `StrategyFileLoadRequested`, `StrategyLoadMode`, `StrategySaveRequested`
- cargo check: components.rs clean。残 23 件は意図的な compile-driven migration エラー（後続ステップで解消）

**知見・設計メモ (Step 1-3)**:
- `PanelSpawnSource` は既に存在していた（Navigator の誤認識あり → 司令塔が修正して Driver に正確な diff を渡した）
- `PendingStrategyLoad` も既に存在していた（削除対象で OK）
- 大規模リファクタリングでは「型定義先行 → compile error が migration漏れを教える」compile-driven migration パターンが有効

### 2026-05-16 Step 4-10 完了ログ + 設計判断・他作業者向け Tips

**Step 4 (floating_window.rs)**:
- `panel_spawn_dispatcher_system` に `ResMut<RegionKeyAllocator>` を追加し、StrategyEditor だけ `matches!(kind, PanelKind::StrategyEditor)` で複数 spawn 許可
- `event.strategy_spec` が `None` の場合は dispatcher で blank デフォルト (`region_key: None, source: Some(String::new()), layout_source: event.source`) を明示構築する方針 — `StrategyEditorSpawnSpec::default()` は意図的に derive しない (sidebar / layout / undo redo それぞれで意味が違うため、デフォルト依存を許さない)
- close button observer は `Query<(.., Option<&StrategyEditorId>, Option<&StrategyFragment>), With<WindowRoot>>` で per-entity に region_key + source を snapshot 化 (singleton 時代の `buffer.source` 参照を完全撲滅)
- `WindowDespawnEdit.strategy_snapshot` は `Option<(String, String)>` ((region_key, source)) になっている — close 観測子で `match (editor_id, fragment) { (Some(id), Some(f)) => Some((id.region_key.clone(), f.source.clone())), _ => None }` パターンで埋める

**Step 5 (strategy_editor.rs per-region sync)**:
- `spawn_strategy_editor_panel` の最終シグネチャ: `(commands, font_system, &mut allocator, spec: StrategyEditorSpawnSpec)`
  - region_key 決定ロジック: `spec.region_key` が `Some(k)` なら `allocator.bump_to_at_least(numeric_suffix_of(&k))` で追従、`None` なら `allocator.allocate()`。**追従しないと sidecar/undo redo で復元した region_005 と allocator.next=1 が衝突して以降の blank spawn が既存と被る**
  - `StrategyEditorId` は **root と editor child の両方に貼る** — `CosmicTextChanged` から region_key を即引きするための意図的二重挿入 (`StrategyFragment` は root のみ = single owner)
- `mark_strategy_dirty` → `mark_fragment_dirty(&mut fragment, &mut auto_save, new_source)` にリネーム — dirty bookkeeping は per-fragment + autosave の 2 ソースに統一
- `sync_strategy_buffer_to_editor_system` は Phase D の置き換えまでの暫定 **no-op stub** (open_events/undo_events を drain するだけ) — Phase D で `RegionTextRestoreRequested` 経由の per-region restore に置き換え予定
- `sync_editor_to_strategy_buffer_system` は `editor_q` (editor child の `StrategyEditorId`) → `fragments_q` (root の `StrategyEditorId + StrategyFragment`) の 2 段 query で hierarchy walk 不要
- **suppress_echo API は `(region_key, text)` ペア化が必須** (`AppHistory::suppress_echo(String, String)` + `suppress_echo_target: Option<(String, String)>`) — fragment 編集 echo を region 単位 + テキスト一致の二重キーで抑止しないと multi-spawn 環境で誤爆する
- `debounced_strategy_autosave_system` は毎フラッシュ時に **全フラグメントを `merge_fragments` で組み立て直す** (No resource caches the merged text 原則)。フラグメント側 `dirty` クリアは現状 `flush_strategy_cache` に含まれていない暫定形 — Phase H で `merge_and_flush_to_cache` 統合時に対応

**Step 6 (sidebar.rs blank-spawn)**:
- ファイルダイアログ (`rfd::FileDialog`) を sidebar から完全撤去 — Open は menu_bar の `File → Open Strategy (.py)...` に集約
- `panel_button_system` から `ResMut<PendingStrategyLoad>` 引数を削除
- `process_pending_strategy_load_system` 関数まるごと削除 — 新設計では dispatcher が spawn 時に同期的に source を流し込むため二フレーム遅延が不要

**Step 7 (menu_bar.rs)**:
- 3 sub-step に分割: 7a 配線, 7b status label fragment 集約, 7c handle_strategy_file_load_system 実装
- File popup 順序: **Strategy 系を先頭、Layout 系を下** にして「Save Strategy ≠ Ctrl+S (Save Layout)」の取り違いを防ぐ。`SaveStrategy { force_dialog: false }` / `SaveStrategyAs { force_dialog: true }` の 1 dispatch with bool flag パターン
- `update_strategy_status_label_system` は `Res<StrategyBuffer> + Query<&StrategyFragment>` で `original_path + cache_path + fragment_count + dirty_count + total_lines` を統合表示。**`buffer.is_changed()` ガードは使えない** (新スキーマでは buffer change が fragment dirty 遷移を検出しないため) → ラベル文字列の等価チェックで書き込み抑制
  - 表示パターン: 「strategy: foo.py cached * [3 regions, 42 lines, 1 dirty]」「strategy: untitled [1 region, 0 lines]」「strategy: none」
- `handle_strategy_file_load_system` の責務 7 段階:
  1. `.py` 読込 (`std::fs::read_to_string`)
  2. `split_py_into_fragments(&source)` で region 分割 (warnings は log で吐く)
  3. `buffer.original_path` / `cache_path` (with sidecar copy) を更新、`last_merged_source = None` でリセット
  4. `allocator.bump_to_at_least(outcome.max_numeric_suffix)`
  5. 既存 StrategyEditor root を全 despawn (`PanelKind::StrategyEditor` で filter、`StrategyEditorId` でなく PanelKind を使うのは spawn 直後の Commands 反映待ちタイミングを避けるため)
  6. `PendingStrategyFragments` を clear→詰める + `loaded_for_path` 更新
  7. (mode, sidecar 存在) で分岐:
     - `LayoutRestore` → 何もしない (apply_layout_system が起点なので)
     - sidecar あり → `LayoutLoadRequested` を発火
     - sidecar なし → region ごとに `PanelSpawnRequested` を直接発火 (strategy_spec に `Some(region_key) + Some(body)` 同梱)
  8. `UserOpen` のときだけ `app_state.last_strategy_path` を保存
- test mod の `test_open_strategy_app_copies_sidecar_and_parses_scenario` は `#[cfg(any())]` で **暫定無効化** — handle_strategy_file_load_system 用に書き直す Step 11 案件

**Step 8 (layout_persistence.rs region-aware)**:
- `apply_layout_system` の panels Query に `Option<&StrategyEditorId>` を挟み、match と to_despawn の両 predicate で「StrategyEditor は (kind, region_key) で同一性判定、他 panel は kind 一致のみ」を実装
- 旧 layout JSON (region_key 不在) は legacy migration として `unwrap_or_else(|| "region_001".to_string())` で扱う — 単一 StrategyEditor だった旧資産は壊れない
- spawn 要求時、StrategyEditor の `strategy_spec` は `region_key: Some(want_key), source: None, layout_source: LayoutLoad` — `source: None` にして dispatcher が `PendingStrategyFragments` を drain する責務分離
- `PendingLayoutLoad` resource と `watch_open_strategy_for_sidecar_system` / `auto_load_sidecar_system` 2 system を **完全削除** — 新フローは `handle_strategy_file_load_system` が UserOpen 時に sidecar を判定して `LayoutLoadRequested` を直接発火する 1-shot 構造
- `SidecarAutoLoadState.done` は意味を変えた: 「apply_layout_system が `strategy_path` 由来の `StrategyFileLoadRequested { LayoutRestore }` を 1 度だけ発火する」ループ防止フラグ
- `build_layout` の panels Query 拡張 → `region_key: id.map(|i| i.region_key.clone())` を WindowLayout に書き出す。**これを忘れると保存→再起動で region_key 不在になり全部 region_001 にマージされる**
- build_layout を呼ぶ 4 system (`handle_save_layout_system` / `handle_save_as_layout_system` / `save_layout_on_window_close` / `debounced_autosave_system`) の panels Query も同型に揃える必要あり

**Step 9 (footer.rs Run)**:
- Run フローは debounced autosave と全く同じ「fragments を region_key 昇順 sort → `merge_fragments` → `flush_strategy_cache(&merged, &mut buffer, &mut auto_save)`」パターン
- dirty 判定は `auto_save.dirty` に統一 (旧 `buffer.dirty` は撲滅済み)
- 計画書 Phase H の `merge_and_flush_to_cache` ラッパーは **Step 10 で抽出予定だったが未対応** — autosave と Run の 2 箇所に同じ merge+flush パターンが現存。**Step 11 候補**

**Step 10 (mod.rs sweep)**:
- 旧 import / 旧 event / 旧 system 名を全撲滅し、新 `RegionKeyAllocator` / `PendingStrategyFragments` を `init_resource`、`StrategyFileLoadRequested` / `StrategySaveRequested` を `add_event`、`handle_strategy_file_load_system` / `log_strategy_file_load_requested_system` を新 `add_systems` 登録
- `sync_strategy_buffer_to_editor_system.after(handle_strategy_file_load_system)` で順序制約を 1:1 置換
- **Bevy 0.15 add_systems タプル 20 個上限**: 現状 mod.rs Update タプルは最大 16 個でゆとりあり。`StrategyLoadSet` 等の SystemSet 導入は計画書 Phase I 通り別 Step
- 副次効果として `apply_buffer_to_editor` ヘルパが現状 dead_code warning (Phase D で復活予定なので意図的に残置)

### Pair-Relay (Navigator/Driver/Verifier) 運用 Tips

- **司令塔は SendMessage が無い環境では fresh spawn で都度 context を埋めて回せばよい** — pair-relay スキル原文は同一 3 体の SendMessage 継続を推奨だが、SendMessage 非搭載環境でも責務分離の効用は十分得られる
- Verifier は「cargo check のエラー数」「カテゴリ別内訳」「ファイル別内訳」「期待との差分」「pass/fail 判定」のフォーマットで報告すると司令塔が即判断できる
- **compile-driven migration 中はベースライン件数を毎 step 司令塔から Verifier に伝える** こと — Verifier 単独では「24 件は意図的 / 1 件新規混入」を区別できない
- Navigator が「方針判断必要」を返してきたら司令塔は加工せず User の言葉でそのまま運ぶ。司令塔の解釈付加は劣化要因
- 大きな Step (Step 5, 7, 8) は Navigator 自身が sub-step に切ってくれる — 1 ターン 200 行超えの diff は集中力低下とレビュー漏れの原因なので 50-150 行に抑える
- Navigator は仮定を明示する責務 (e.g., 「editor_history.rs は Step 2 で region_key 化済みと仮定」) — 仮定が外れたら Verifier が cargo check で検出 → 司令塔が Navigator に追加修正を依頼
- **Driver は "write-only typist"**: 範囲拡大・調査・提案を Driver に許すと暗黙改変で Verifier が原因特定できなくなる。Driver の出力は「触ったファイル + 行範囲 + やったこと」の 3-5 行報告に絞ること

### Step 11 候補 (引き継ぎ事項)

1. **test ビルド 29 件失敗**: `src/ui/strategy_editor.rs` の `#[cfg(test)] mod tests` が旧 `StrategyBuffer.source` / `dirty` フィールドを参照したまま。新スキーマ (`original_path` / `cache_path` / `last_merged_source` + `StrategyFragment` per entity) に追従する書き直しが必要
   - カテゴリ: E0560/E0609 ×17 (source, dirty フィールド消滅), E0061 ×3 (関数引数数), E0063 ×1 (AppEditAction.region_key), E0425 ×1 (mark_strategy_dirty 未定義), E0308 ×1 (型不一致), その他 6
2. **menu_bar.rs の `test_open_strategy_app_copies_sidecar_and_parses_scenario` 復活**: `#[cfg(any())]` で disable 中。`handle_strategy_file_load_system` + `parse_scenario_system` の二本立て統合テストとして書き直し
3. **`merge_and_flush_to_cache` ラッパー抽出**: debounced autosave (`strategy_editor.rs::debounced_strategy_autosave_system`) と footer Run (`footer.rs::footer_pause_resume_system` 内) の 2 箇所に同じ「fragments 集約 → merge_fragments → flush_strategy_cache」パターンが現存。Phase H の統合ラッパーに集約する
4. **`sync_strategy_buffer_to_editor_system` 削除**: 現状 EventReader を 2 つ持って `.clear()` するだけの no-op stub。Phase D で `RegionTextRestoreRequested` 経由の per-region restore を導入したら本 system は不要
5. **`apply_buffer_to_editor` ヘルパの行く先**: dead_code warning 中。Phase D で復活するか、不要なら削除
6. **clippy warning 整理**: `collapsible_if` / `collapsible_else_if` 系 9 件 (任意)
7. **`PanelSpawnRequested.strategy_spec` の `source: None` drain 経路の Verifier**: layout 復元時 `source: None` で spawn → dispatcher が `PendingStrategyFragments` から `region_key` で drain する経路の動作テスト未実施

### 既知 caveat (新スキーマで踏むと痛い罠)

- `StrategyEditorId` を root と editor child の両方に貼ること。片方だけだと `CosmicTextChanged` から region_key を引けず singleton 時代の echo 抑止に逆戻りする
- spawn 時の region_key 払い出しで `allocator.bump_to_at_least` を呼び忘れると、後続 blank spawn が既存と衝突する
- `flush_strategy_cache` のシグネチャは `(merged: &str, &mut StrategyBuffer, &mut StrategyAutoSaveState) -> std::io::Result<bool>` — 旧 `(&mut StrategyBuffer, &mut StrategyAutoSaveState)` シグネチャを呼ぶと E0061
- `WindowLayout.region_key: Option<String>` — None は 「StrategyEditor 以外 or legacy layout JSON」を意味する。新規 StrategyEditor 保存時は必ず Some を埋める
- Bevy 0.15 `add_systems` タプル 20 個上限。`(a, b, ..., t).chain()` の境界で `invalid system configuration` という分かりにくいエラーになる。新 system 追加時は数える
- CosmicEdit テキスト注入は `CosmicEditBuffer::with_text` だけでなく `CosmicEditor.with_buffer_mut(|b| { b.set_text(...); b.set_redraw(true); })` も必要 (現状は spawn 時の `with_text` だけで足りているが、後段で per-region restore を実装する際は両方必要)
- pair-relay スキル原文の SendMessage ベース運用は Claude Code 環境によっては SendMessage tool 自体が未搭載のことがある。代替として fresh spawn + 文脈再注入で同等品質を担保できる

## Context

Current state: Strategy Editor is a singleton. Sidebar button (`src/ui/sidebar.rs:195-219`) doubles as "Open .py" — one click opens a file dialog and spawns a single editor. `StrategyBuffer.source: String` (`src/ui/components.rs:99-104`) holds the entire `.py` content, and the dispatcher (`src/ui/floating_window.rs:236-241`) blocks duplicate StrategyEditor spawns.

Target:
- Sidebar "Strategy Editor" button = **spawn an empty editor**. No limit, no file dialog.
- **Untitled (no `original_path`) lifecycle** (review Severe #1, #2):
  - First blank spawn while `buffer.original_path == None` synthesizes an `untitled` cache path: `dirs::cache_dir()/the-trader-was-replaced/strategy_buffers/untitled__<unix_epoch_secs>.py` and stores it as `buffer.cache_path`. Subsequent blank spawns reuse the same cache path (they merge into the same file). This **removes the "no cache_path → Run blocked" failure mode**; the Run command still requires `ScenarioMetadata` to be set (`menu_bar.rs:420`), and that gate is unchanged (review 高 #5).
  - Save As success path: the previous `cache_path` was `untitled__<old>.py`. After the dialog returns a real `.py`, recompute `cache_path` via `strategy_cache_path(new_original)` and **delete the old untitled cache file** (`fs::remove_file`, ignore NotFound). This keeps `strategy_buffers/` from accumulating untitled stragglers (review 中 #7). If a user opens a real `.py` directly (without going through Save As), no cleanup is needed because untitled was never created in that session.
  - `File → Save Strategy (.py)` writes to `buffer.original_path` if set; if `original_path == None`, it falls through to a Save As file dialog and updates `original_path`/`cache_path` (via existing `strategy_cache_path` hash logic) before writing. A separate `File → Save Strategy As...` (`MenuItem::SaveStrategyAs`) is always-dialog. Two menu items, but the dispatch logic is a single function with a `force_dialog: bool` flag.
- **"Save" semantics — keep two write paths distinct** (review High #1):
  - **autosave** writes the merged `.py` to **`cache_path`** every 1s after edits. This is the backend-readable artifact for Run. Existing `Ctrl+S = MenuItem::SaveLayout` (`menu_bar.rs:104`) stays for **layout JSON only** — unchanged.
  - **explicit Save Strategy** (new menu entry `File → Save Strategy (.py)`) writes the merged `.py` to **`buffer.original_path`** for the user-visible source file. `# region`/`# endregion` markers are emitted here too. After a successful explicit save, also call `merge_and_flush_to_cache` so `cache_path` is kept in lockstep, and clear all `StrategyFragment.dirty` flags so the status label and autosave timer reset (review 高 #6: prevents "I just saved but the status still says dirty"). `StrategyAutoSaveState` is reset the same way the autosave path resets it.
- Load: split a `.py` on `# region`/`# endregion`, despawn existing editors, spawn N editors each holding one fragment (marker lines themselves are stripped from the in-memory source and from the editor view).
- Run: backend reads the merged `cache_path` `.py` — `handle_strategy_run_system` itself unchanged, but **every call site of `flush_strategy_cache` must be replaced with `merge_and_flush_to_cache`** (`rg flush_strategy_cache` is the source of truth; documented call sites are footer Run and debounced autosave — review Medium #8).
- Sidecar JSON records each StrategyEditor as a separate `WindowLayout` with `region_key`, so reload restores both placement and content.

## Design decisions (confirmed by user)

| Topic | Decision |
|---|---|
| Source storage | **Per-entity** `StrategyFragment { source, dirty }` on the root window entity. **No resource caches the merged text**. `merge_and_flush_to_cache` builds the merged string locally each call and writes to disk; `StrategyBuffer.source` is removed. A read-only `last_merged_source: Option<String>` field on `StrategyBuffer` (set only on successful flush) is kept for the status label and tests. |
| Merge order | **`region_key` ascending** (string compare). Stable across drags; drag-reorder is deferred. (Review Medium #7.) |
| region_key | Auto-numbered `region_001`, `region_002`, ...; rename UI deferred. |
| `.py` without any markers | Treat as one region (`region_001`, full file body). Next save re-emits markers. |
| Undo `SetStrategySource` | Region-scoped: `SetStrategySource { region_key, text }`. If no editor matches, log and skip. |
| **dirty bookkeeping** | Two sources only: per-fragment `StrategyFragment.dirty` (set by editor input / undo) and `StrategyAutoSaveState { dirty, last_change }` (debounce timer). `StrategyBuffer.dirty` is removed. Successful merge flush clears all fragments' `dirty` and `auto_save.dirty`. |

## Identity model — resolves the "root vs editor entity" ambiguity (review High #3)

Two-entity layout per StrategyEditor:
- **root** (`WindowRoot` + `PanelKind::StrategyEditor` + `StrategyEditorId { region_key }` + `StrategyFragment { source, dirty }`)
- **editor child** (`StrategyEditorContent` + `StrategyEditorId { region_key }`, parented to `content_area` of root)

Rule: **`StrategyEditorId` is the routing key**, placed on both nodes so any system can locate root or child from a region_key without a hierarchy walk. `StrategyFragment` lives **only on root** — single owner. `CosmicTextChanged` resolves editor→root by querying `Query<&StrategyEditorId, With<StrategyEditorContent>>` then `Query<(Entity, &StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>` matching `region_key`.

## Data types

```rust
// src/ui/components.rs (or src/ui/strategy_editor.rs)
#[derive(Component, Debug, Clone)]
pub struct StrategyEditorId { pub region_key: String }

#[derive(Component, Debug, Clone, Default)]
pub struct StrategyFragment { pub source: String, pub dirty: bool }

#[derive(Resource, Default)]
pub struct RegionKeyAllocator { pub next: u32 }
impl RegionKeyAllocator {
    pub fn allocate(&mut self) -> String { self.next += 1; format!("region_{:03}", self.next) }
    pub fn bump_to_at_least(&mut self, n: u32) { self.next = self.next.max(n); }
}

// `StrategyBuffer` keeps only path metadata + a read-only "last merged" view.
// No cache resource exists for the merged text — it is rebuilt on each flush.
//   pub struct StrategyBuffer {
//       pub original_path: Option<PathBuf>,
//       pub cache_path: Option<PathBuf>,
//       pub last_merged_source: Option<String>,  // written ONLY by merge_and_flush_to_cache
//   }
// `dirty` is gone (lives on fragments + StrategyAutoSaveState).
```

**Source-bearing pending resource** — needed because `WindowLayout` does not carry source text:

```rust
#[derive(Resource, Default, Debug)]
pub struct PendingStrategyFragments {
    /// region_key -> source body (no markers, no trailing \n).
    pub by_region_key: std::collections::HashMap<String, String>,
    /// The .py path this batch was parsed from. Drains reject mismatches and warn —
    /// guards against an in-flight pending set from a previous load colliding with the next one.
    pub loaded_for_path: Option<std::path::PathBuf>,
}
```
Populated by `handle_strategy_file_load_system` (only on the `LayoutRestore` branch and the sidecar-exists branches of UserOpen/StartupRestore). `panel_spawn_dispatcher_system` drains an entry by region_key when `strategy_spec.source.is_none() && strategy_spec.region_key.is_some()`. Drains on use so stale entries don't leak. If a drain miss occurs (key absent), spawn blank and `warn!`.

**Unified file-load event** (review High #2) — collapses the dual `OpenStrategyRequested` consumers:

```rust
#[derive(Event, Debug, Clone)]
pub struct StrategyFileLoadRequested {
    pub path: std::path::PathBuf,
    pub mode: StrategyLoadMode,
}
#[derive(Debug, Clone, Copy)]
pub enum StrategyLoadMode {
    /// User chose Open Strategy menu item — replace everything, then apply sidecar if it exists.
    UserOpen,
    /// Layout JSON's strategy_path field — sidecar layout already decides window placement; source map comes from the .py but layout decides spawns.
    LayoutRestore,
    /// app_state last_strategy_path on startup — same as UserOpen but suppresses the sidecar autoload state machine echo.
    StartupRestore,
}
```
The legacy `OpenStrategyRequested` event is replaced by `StrategyFileLoadRequested`. A single handler `handle_strategy_file_load_system` owns the read → split → fragment table → spawn-or-layout-apply decision tree based on `mode`. This removes the current race where `open_strategy_buffer_system` and the sidecar watcher (`layout_persistence.rs:494`) both react to the same event independently.

Spawn argument struct:
```rust
pub struct StrategyEditorSpawnSpec {
    pub region_key: Option<String>,   // None => allocator.allocate()
    pub source: Option<String>,        // None => drain PendingStrategyFragments[region_key]; Some("") => explicit blank; Some(s) => use s
    pub layout_source: PanelSpawnSource,
}
```
Threaded through `PanelSpawnRequested` as `pub strategy_spec: Option<StrategyEditorSpawnSpec>` (None for non-StrategyEditor panels and for sidebar-blank spawn where dispatcher fills defaults: `region_key = None → allocate`, `source = Some(String::new())`).

## Implementation phases

### Phase A — Identity & storage refactor (no UI behavior change yet)

1. Add `StrategyEditorId`, `StrategyFragment`, `~~MergedStrategyCache~~ (rejected; merged text is built locally in `merge_and_flush_to_cache`)`, `RegionKeyAllocator`.
2. `spawn_strategy_editor_panel(commands, font_system, allocator, spec)` allocates/uses region_key, inserts the two components on root and id on editor child, seeds cosmic buffer with `spec.source`.
3. Rewrite `sync_editor_to_strategy_buffer_system` (`src/ui/strategy_editor.rs:259`) to:
   - Read `CosmicTextChanged(editor_entity, new_text)`.
   - Look up editor's `StrategyEditorId.region_key`.
   - Find matching root and update its `StrategyFragment.source/dirty`.
   - Set `StrategyAutoSaveState` (kept global).
4. Rewrite `sync_strategy_buffer_to_editor_system` (`src/ui/strategy_editor.rs:222`) to per-region: drive set_text only for editors whose region_key matches the event (new `RegionTextRestoreRequested { region_key, text }` event; existing `OpenStrategyRequested` keeps file-level semantics but now fans out into per-region restore requests, see Phase D).
5. Replace all `StrategyBuffer.source` reads/writes:
   - Cache flush: `merge_and_flush_to_cache(query, &mut buffer, &mut auto_save)` walks fragments in **`region_key` ascending order** (matches the decision in the table above), builds merged text, writes to `buffer.cache_path`.
   - `update_strategy_status_label_system` (`menu_bar.rs:319`) reads cache.dirty + fragment count.

**Newline normalization** (review Medium #9): on split, each fragment body's trailing `\n` is stripped. On merge, each fragment body is emitted then a `\n` separator then `# endregion <key>\n`. cosmic_edit's editor source always carries a trailing `\n` in normal typing — the sync system (`sync_editor_to_strategy_buffer_system`) normalizes the incoming text by `trim_end_matches('\n')` before storing in `StrategyFragment.source`. The cosmic editor view is not modified (display preserves trailing newline as the user typed it); only the persisted/round-tripped representation is normalized. Round-trip invariant: `split(merge(xs)) == xs` for any `xs` where every body is already normalized (no trailing `\n`).

### Phase B — Merge / split helpers (pure functions, unit-testable)

```rust
// src/ui/strategy_editor.rs (or new src/ui/strategy_merge.rs)
pub fn merge_fragments(items: &[(String /*key*/, String /*src*/)]) -> String;
pub fn split_py_into_fragments(py: &str) -> SplitOutcome;

pub struct SplitOutcome {
    pub fragments: Vec<(String, String)>,   // ordered
    pub max_numeric_suffix: u32,             // for RegionKeyAllocator.bump_to_at_least
    pub warnings: Vec<String>,               // logged via warn!
}
```

Parser rules (review Medium #7):
| Case | Behavior |
|---|---|
| Zero markers in file | One fragment `("region_001", full_text)`. warnings empty. |
| `# region <key>` ... `# endregion <key>` matched pairs | Standard split. Marker lines dropped from fragment source. |
| `# endregion` with a different key than open | Close current region anyway, push warning. |
| `# region` while another is still open | Close previous implicitly at the new `# region` line, push warning. |
| Trailing `# region` with no `# endregion` | Close at EOF, push warning. |
| Preamble lines before first `# region` | Wrap into synthetic `("region_001_preamble", text)` and bump allocator past it. warning. |
| Duplicate region_key | Suffix later occurrences with `_dupN` (e.g. `region_001_dup1`) and warn. |

`merge_fragments` always emits exact `# region <key>\n...body...\n# endregion <key>\n` per item, with no trailing blank line between regions (one `\n` separator only).

Round-trip property: `split(merge(x)) == x` for any sequence of `(key, body_without_trailing_newline)` where `key` is unique and matches `^[A-Za-z_][A-Za-z0-9_]*$`. Tested.

### Phase B.5 — Echo suppression: region-aware (review Medium #5)

`AppHistory::suppress_echo_target` (`strategy_editor.rs:273`) becomes `Option<(String /*region_key*/, String /*text*/)>`. `sync_editor_to_strategy_buffer_system` matches both region_key (looked up from the editor entity's `StrategyEditorId`) AND text before consuming the echo. `TextEdit` history command itself (`editor_history.rs:60`) gains `pub region_key: String` so undo/redo can target the right editor.

### Phase C — Sidebar: blank-spawn, no file dialog

`src/ui/sidebar.rs:195-219`: delete the StrategyEditor branch entirely. Replace the generic branch's duplicate check with:
```rust
let allow_multi = matches!(kind, PanelKind::StrategyEditor);
if allow_multi || !existing_kinds.iter().any(|k| k == kind) {
    spawn_events.send(PanelSpawnRequested {
        kind: *kind, source: PanelSpawnSource::User,
        strategy_spec: None,
    });
}
```
**Cleanup of `PendingStrategyLoad`** (review Medium #7): delete `PendingStrategyLoad`, `process_pending_strategy_load_system`, and `PendingStrategySnapshotRestore`. They are all artifacts of the singleton "spawn-then-stream-content" design. In the new flow:
- File-content load comes through `StrategyFileLoadRequested` with `mode = LayoutRestore` (driven by `apply_layout_system`'s `strategy_path` field).
- The "wait for editor entity then apply text" two-frame delay is replaced by `PendingStrategyFragments` (declared above) — `panel_spawn_dispatcher_system` consumes it synchronously at spawn time, so no waiting is needed.
- Undo-spawn carries its restore source via `WindowSpawnEdit.strategy_snapshot: Option<(region_key, source)>` directly in the spawn spec (no separate pending resource).

### Phase D — Open .py: file loader owns split, layout system owns placement

Ownership rule (one owner per concern):
- **`handle_strategy_file_load_system`** owns `.py` read + split + `PendingStrategyFragments` population + path/cache metadata.
- **`apply_layout_system`** owns spawn placement when a sidecar JSON exists (drains pending by region_key).
- **No path is parsed twice**. No two systems issue spawn requests for the same load.

`UserOpen` does **not** consult any `strategy_path` field inside a sidecar (review 重大 #2). The user explicitly chose this `.py`; that path is the source of truth. The sidecar at `<path>.json`, if present, only contributes window placement and the StrategyEditor `region_key`→window mapping. A `strategy_path` inside the sidecar that disagrees with the opened path is ignored (warn-log on mismatch).

- New `File` menu entries (`src/ui/menu_bar.rs:101-106`):
  - `"Open Strategy (.py)..."` → `MenuItem::OpenStrategy` → file dialog → emits `StrategyFileLoadRequested { path, mode: UserOpen }`.
  - `"Save Strategy (.py)"` → `MenuItem::SaveStrategy` → emits `StrategySaveRequested { force_dialog: false }`. Handler writes to `original_path`; if None, opens Save As dialog and updates path metadata before writing.
  - `"Save Strategy As..."` → `MenuItem::SaveStrategyAs` → emits `StrategySaveRequested { force_dialog: true }`.
- `handle_strategy_file_load_system` (replaces `open_strategy_buffer_system`):
  1. Read .py text; `split_py_into_fragments` → `SplitOutcome`.
  2. Set `buffer.original_path`, `buffer.cache_path` (existing hash logic), clear `last_merged_source`.
  3. `allocator.bump_to_at_least(outcome.max_numeric_suffix)`.
  4. Despawn all existing StrategyEditor roots.
  5. Populate `PendingStrategyFragments.by_region_key` from the split outcome.
  6. Branch on `(mode, sidecar_exists)` where `sidecar_exists = path.with_extension("json").exists()`:
     | mode | sidecar | action |
     |---|---|---|
     | `UserOpen` / `StartupRestore` | yes | emit `LayoutLoadRequested { path: sidecar }`. `apply_layout_system` owns spawn placement and drains `PendingStrategyFragments`. |
     | `UserOpen` / `StartupRestore` | no | direct cascade spawn: one `PanelSpawnRequested` per fragment with `strategy_spec.source = Some(body)` (no pending drain needed in this branch). |
     | `LayoutRestore` | n/a | do nothing further. `apply_layout_system` already invoked the load and will drain pending on its own spawn requests. |
  7. Persist `app_state.last_strategy_path` for `UserOpen` only.

`restore_last_strategy_system` (`menu_bar.rs:283`) emits `StrategyFileLoadRequested { mode: StartupRestore }`. `SidecarAutoLoadState` (`layout_persistence.rs:101`) continues to gate the startup one-shot.

### Phase E — Dispatcher & spawn wiring

`src/ui/floating_window.rs:229`:
- `panel_spawn_dispatcher_system` reads `event.strategy_spec` for StrategyEditor; calls `spawn_strategy_editor_panel(commands, font_system, allocator, spec_or_default)`.
- Duplicate check `already && !matches!(event.kind, PanelKind::StrategyEditor)`.
- `WindowSpawnEdit` push: include `region_key` and the spawning fragment.source so undo-spawn can reconstruct it (see Phase F).

Close button observer (`src/ui/floating_window.rs:196`):
- For StrategyEditor close, snapshot the closing root's `StrategyFragment.source` (not `buffer.source`, which no longer exists). Query gains `Option<&StrategyEditorId>` and `Option<&StrategyFragment>`. Add to `WindowDespawnEdit`:
  ```rust
  pub struct WindowDespawnEdit {
      pub layout: WindowLayout,                // includes region_key
      pub strategy_snapshot: Option<(String, String)>,  // (region_key, source)
  }
  ```

Drag observer in `spawn_floating_window` (title bar `Pointer<Drag>` handler) — review Medium #6:
- Same treatment: query gains `Option<&StrategyEditorId>` so `WindowMoveEdit` records `region_key` for StrategyEditor drags. Without this, multi-editor drag undo collapses all StrategyEditor windows onto one history entry.

### Phase F — layout_persistence: region-aware lookup (review High #1)

`WindowLayout` adds `region_key: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` so existing sidecar JSON without the field deserializes. **Migration fallback** (review Severe #3): when loading an old sidecar that has a `StrategyEditor` entry with `region_key == None`, treat it as `Some("region_001")`. This pairs with legacy `.py` files (also lacking markers) being parsed into a single `region_001` fragment, so old layout + old .py still restore cleanly.

`apply_layout_system` (`src/ui/layout_persistence.rs:316-358`):
- Change panel match key from `**kind == win_layout.kind` to:
  ```rust
  fn matches(layout: &WindowLayout, kind: &PanelKind, id: Option<&StrategyEditorId>) -> bool {
      if *kind != layout.kind { return false; }
      match (layout.kind, layout.region_key.as_deref(), id) {
          (PanelKind::StrategyEditor, Some(k), Some(eid)) => eid.region_key == k,
          (PanelKind::StrategyEditor, _, _) => false, // require region_key for SE
          _ => true,
      }
  }
  ```
- Same predicate in the despawn-orphans pass (`layout_persistence.rs:351`).
- Spawn path: when sending `PanelSpawnRequested` for a missing StrategyEditor, attach `strategy_spec = Some(StrategyEditorSpawnSpec { region_key: Some(layout.region_key.unwrap_or("region_001")), source: None, layout_source: LayoutLoad })`. `source: None` triggers a `PendingStrategyFragments` drain in the dispatcher.

**Layout autosave dirty** — every cause of window-list change:
- `panel_spawn_dispatcher_system`: after a StrategyEditor spawn whose `source == User`, set `AutoSaveState.dirty = true`.
- Close observer: after pushing `WindowDespawnEdit` for a StrategyEditor, set `AutoSaveState.dirty = true`.
- `apply_pending_app_edits_system` (review 高 #4): when a `SpawnWindow`/`DespawnWindow` action mutates the world during Undo/Redo, set `AutoSaveState.dirty = true`. Without this hook, undo/redo of window ops doesn't trigger sidecar save and the next launch sees stale layout.

Sidecar source bodies: `WindowLayout` does **not** carry source text (avoid duplicating `.py` content in JSON). On load, `apply_layout_system` triggers `StrategyFileLoadRequested { mode: LayoutRestore }`, which populates `PendingStrategyFragments`. `apply_layout_system` then emits per-region `PanelSpawnRequested` entries, and `panel_spawn_dispatcher_system` drains `PendingStrategyFragments[region_key]` to seed the spawned fragment. If a layout entry references a region_key absent from the `.py`, log warn and spawn a blank editor for that key (allocator already bumped).

`build_layout` (`layout_persistence.rs:125`) query gains `Option<&StrategyEditorId>` so each StrategyEditor root contributes its `region_key` to the serialized `WindowLayout`.

`apply_pending_layout_system` (`layout_persistence.rs:385`): same region-aware match.

### Phase G — Undo/Redo: region-scoped (review High #2)

`src/ui/editor_history.rs:29-39`:
```rust
pub enum AppEditAction {
    SetStrategySource { region_key: String, text: String },
    MoveWindow { kind: PanelKind, region_key: Option<String>, position: Vec2 },
    SpawnWindow { layout: WindowLayout, strategy_snapshot: Option<(String, String)> },
    DespawnWindow { kind: PanelKind, region_key: Option<String> },
}
```

`apply_pending_app_edits_system` (`strategy_editor.rs:354`):
- For each variant, locate the root entity via `(PanelKind, Option<region_key>)` predicate built on top of `StrategyEditorId`.
- `SetStrategySource`: find root by `region_key` and update `StrategyFragment.source` (mark dirty), then fire a per-region restore so the cosmic editor reflows.
- `MoveWindow` / `DespawnWindow`: if target root not found, `warn!` and skip (do not silently no-op without logging).
- `SpawnWindow`: pass `strategy_snapshot` through `strategy_spec` into the `PanelSpawnRequested`.

`history.push_*` call sites need to pass `region_key` along — driven from where the action originates (editor entity, window root entity).

### Phase H — Merge flush: single helper, every entry point

Centralized helper (no cache resource — builds string locally each call):
```rust
pub fn merge_and_flush_to_cache(
    fragments_query: impl IntoIterator<Item = (&Transform, &StrategyEditorId, &mut StrategyFragment)>,
    buffer: &mut StrategyBuffer,
    auto_save: &mut StrategyAutoSaveState,
) -> std::io::Result<bool>;
// Returns Ok(true) on success (cache_path written, fragments dirty cleared, last_merged_source set),
//         Ok(false) if cache_path is None,
//         Err on I/O failure (state untouched, same retry semantics as old flush_strategy_cache).

pub fn merge_and_save_strategy(
    fragments_query: ...,
    buffer: &mut StrategyBuffer,
) -> std::io::Result<bool>;
// Variant that writes to buffer.original_path for explicit File → Save Strategy.
// Does NOT touch auto_save or fragment.dirty (autosave continues to own dirty).
```

Pre-implementation: `rg flush_strategy_cache` to confirm all call sites. Documented today: `strategy_editor.rs:453` (debounce) and `footer.rs:467` (Run button). The comment at `footer.rs:460` about "Run observer in strategy_editor" appears stale — no separate observer exists.

Replace **all** call sites with `merge_and_flush_to_cache` (always run on Run button — drop the `if buffer.dirty` guard since dirty now lives on fragments). Backend `handle_strategy_run_system` (`menu_bar.rs:414`) is unchanged.

### Phase I — System ordering (across `src/ui/mod.rs` AND `layout_persistence` plugin)

Both UiPlugin (`src/ui/mod.rs`) and the layout plugin add systems. The two plugins must agree on order to keep the load pipeline race-free. Use explicit `.before/.after` and a single named `SystemSet` rather than relying on plugin registration order.

Pipeline (every load cycle):
```
handle_strategy_file_load_system       // reads .py, fills PendingStrategyFragments, sets metadata
    .in_set(StrategyLoadSet::Parse)
apply_layout_system                    // sends PanelSpawnRequested for each WindowLayout, drains pending
    .in_set(StrategyLoadSet::SpawnRequest)
    .after(StrategyLoadSet::Parse)
panel_spawn_dispatcher_system          // actually spawns, drains PendingStrategyFragments at spawn time
    .in_set(StrategyLoadSet::Spawn)
    .after(StrategyLoadSet::SpawnRequest)
apply_pending_layout_system            // applies positions to spawned roots
    .after(StrategyLoadSet::Spawn)
```
Other orderings:
- `panel_button_system.before(panel_spawn_dispatcher_system)`.
- `sync_editor_to_strategy_buffer_system.before(debounced_strategy_autosave_system)` — same-frame edits flush.
- `apply_pending_app_edits_system.before(debounced_strategy_autosave_system)` — Undo/Redo fragment writes participate in this frame's autosave.

`StrategyLoadSet` is a new `SystemSet` defined in `src/ui/strategy_editor.rs` (re-exported as needed) so both the UiPlugin and the layout plugin can configure their systems into it. This is the only structural change to plugin wiring beyond `add_systems` calls.

## Cleanups

- **Delete (definitions + every reference)**:
  - `OpenStrategyRequested` event type and add_event registration — `src/ui/mod.rs:23` neighborhood.
  - `log_open_strategy_requested_system` — `src/ui/menu_bar.rs:295` (rename to `log_strategy_file_load_requested_system` for the new event, or drop entirely if no callers need the trace).
  - `PendingStrategyLoad` resource + `process_pending_strategy_load_system` — `src/ui/sidebar.rs:188` (resource ref), `src/ui/sidebar.rs:238` (system body), `src/ui/mod.rs:114` (system registration).
  - `PendingStrategySnapshotRestore` resource + `apply_strategy_snapshot_restore_system` + its tests — `src/ui/strategy_editor.rs:424`, `src/ui/editor_history.rs:322` (resource decl).
- **Remove fields**: `StrategyBuffer.source`, `StrategyBuffer.dirty`. Add `StrategyBuffer.last_merged_source: Option<String>` (read-only externally).
- **Sweep**:
  - `rg "buffer\.source|buffer\.dirty"` — must be empty. Any remaining hit is a missed field-access migration.
  - `rg "StrategyBuffer \{"` — expected matches only in the struct definition, `Default` impl, and test fixtures that initialize the new schema. Audit each match by hand.
  - `rg "OpenStrategyRequested|PendingStrategyLoad|PendingStrategySnapshotRestore|MergedStrategyCache"` — must be empty.
- **Tests**: `flush_returns_*` (`strategy_editor.rs:478-549`) rewritten against `merge_and_flush_to_cache` taking a synthetic fragments iterator. `apply_pending_app_edits_sets_autosave_dirty_on_strategy_source_action` (`strategy_editor.rs:612`) keeps shape but now spawns a `StrategyEditorContent` with `StrategyEditorId` and asserts on `StrategyFragment.dirty`. Drop tests tied to `PendingStrategySnapshotRestore`.

### Downstream sanity checks

- `scenario_parser.rs`: depends on `StrategyBuffer.original_path` change detection (not `source`). Open flow change preserves `original_path` writes — verify with `Grep "buffer\.original_path"` after Phase D edits.
- `editor_history::TextEdit` command body itself takes `region_key: String` (not only `AppEditAction`). All call sites of `history.push_text(...)` must be updated to pass region_key, which now comes from the `StrategyEditorId` resolved during `sync_editor_to_strategy_buffer_system`.
- `src/camera.rs:pancam_suppression_over_editor_system` queries `With<StrategyEditorContent>` (review 中 #9). Multi-editor doesn't change the component name or count expectations — the suppression triggers when the cursor hovers **any** editor — but confirm via `Grep "StrategyEditorContent"` that no system assumes uniqueness (e.g. `query.get_single()`). If any does, switch to `query.iter()`.

## Files changed

| File | Purpose |
|---|---|
| `src/ui/components.rs` | `PanelKind` unchanged; add `StrategyEditorId`, `StrategyFragment`, `RegionKeyAllocator`, `PendingStrategyFragments`, `StrategyFileLoadRequested`/`StrategyLoadMode`, `StrategySaveRequested`; add `strategy_spec` field on `PanelSpawnRequested`; remove `StrategyBuffer.source`/`dirty` and add `last_merged_source`; delete `OpenStrategyRequested`, `PendingStrategyLoad`, `PendingStrategySnapshotRestore`. |
| `src/ui/strategy_editor.rs` | spawn API takes `StrategyEditorSpawnSpec`; merge/split helpers; per-region sync systems; `merge_and_flush_to_cache`; rewrite of `apply_pending_app_edits_system`. |
| `src/ui/floating_window.rs` | dispatcher honors strategy_spec; duplicate check exempts StrategyEditor; close observer snapshots per-region source. |
| `src/ui/sidebar.rs` | blank-spawn branch; delete file-dialog code and the entire StrategyEditor-specific arm; remove `PendingStrategyLoad` resource argument and `process_pending_strategy_load_system`. |
| `src/ui/menu_bar.rs` | add `MenuItem::OpenStrategy`/`SaveStrategy`/`SaveStrategyAs` + File popup rows; rewrite `open_strategy_buffer_system` → `handle_strategy_file_load_system` (split+multi-spawn or layout-handoff); add `handle_strategy_save_system` (writes `original_path`); `update_strategy_status_label_system` reads `StrategyBuffer.last_merged_source` and fragment count. |
| `src/ui/layout_persistence.rs` | `WindowLayout.region_key`; region-aware lookup in `apply_layout_system` and `apply_pending_layout_system`; build_layout enumerates StrategyEditor roots individually. |
| `src/ui/editor_history.rs` | `AppEditAction` variants gain `region_key`; `WindowDespawnEdit.strategy_snapshot: Option<(String,String)>`. |
| `src/ui/footer.rs` | Run flush goes through `merge_and_flush_to_cache`. |
| `src/ui/mod.rs` | register new systems/resources; order updates. |

## Reusable existing implementations

- `spawn_floating_window` (`src/ui/floating_window.rs`) — regulation #1, used unchanged for the chrome.
- `strategy_cache_path` (`src/ui/menu_bar.rs:301`) — one hashed cache file per original `.py` is the right granularity (merged .py is what backend reads).
- `copy_sidecar_to_cache` (`src/ui/menu_bar.rs:382`) — unchanged.

## Verification

1. `cargo check` clean.
2. `cargo test` — existing tests adapted; new unit tests:
   - `merge_fragments_round_trips_through_split`
   - `split_py_handles_no_markers_returns_single_region`
   - `split_py_handles_preamble_warns_and_wraps`
   - `split_py_handles_duplicate_region_keys`
   - `split_py_handles_unmatched_endregion`
   - `split_py_handles_orphan_region_at_eof`
   - `region_key_allocator_bump_to_at_least`
3. Manual E2E (`cargo run`):
   - Click sidebar Strategy Editor 3 times → 3 empty windows, region_001..003.
   - Type code into each, wait 1s → `cache_path` shows three `# region region_00X` blocks in region_key ascending order (regardless of window screen position).
   - Move one window above another, edit, wait 1s → ordering in cache_path is **unchanged** (region_key ascending; window position does not affect merge order).
   - Run via footer Run button mid-edit (before debounce fires) → backend log shows latest merged content.
   - File → Open Strategy on an existing legacy `.py` (no markers) → 1 editor with full content; save → file gets `# region region_001` / `# endregion region_001` markers.
   - Quit and relaunch → all editors restored at their positions, region_keys match.
   - Ctrl+Z after editing region_002 → only region_002 reverts, region_001/003 untouched.
   - Close region_002, Ctrl+Z → region_002 respawns at its prior position with its prior source.

## Out of scope (future phases)

- Rename region_key via UI (title bar double-click).
- Region reordering by drag handle (current y-position ordering is the workaround).
- Region locking / collapsed state in cache JSON.

## Encoding note

This file is written UTF-8 (no BOM). If a viewer shows mojibake, configure it for UTF-8; PowerShell 5.1 `Get-Content` defaults to system codepage and may misrender — use `Get-Content -Encoding UTF8`.
