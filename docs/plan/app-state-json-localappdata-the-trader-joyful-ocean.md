# Plan: Issue B — 起動時 InstrumentRegistry を cache JSON から直接 seed する

## Context

`app_state.json` の `scenario.instruments` は正しく保存されているのに、起動後に原本サイドカー（`pair_trade_minute.json`）の値で上書きされる。

原因: `apply_cache_restore_system` が `buffer.original_path = strategy_path` をセットする → `parse_scenario_system` が `original_path.with_extension("json")` を読む → 原本サイドカーの instruments で registry が置き換わる。

ユーザー方針: **起動時は `app_state.json` / `app_state.py` だけ使う。余計なことしない。**

## 修正箇所

**1ファイルのみ**: `src/ui/layout_persistence.rs` の `apply_cache_restore_system`

## 変更内容

### Step 1: 関数シグネチャに3リソースを追加

```rust
fn apply_cache_restore_system(
    mut events: EventReader<CacheRestoreRequested>,
    mut buffer: ResMut<StrategyBuffer>,
    mut allocator: ResMut<RegionKeyAllocator>,
    mut pending_fragments: ResMut<PendingStrategyFragments>,
    mut camera: Query<...>,
    mut pending: ResMut<PendingLayoutApply>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    // ↓ 追加
    mut registry: ResMut<crate::ui::components::InstrumentRegistry>,
    mut scenario: ResMut<crate::ui::components::ScenarioMetadata>,
    mut watch: ResMut<crate::ui::components::ScenarioFileWatchState>,
    mut writeback: ResMut<crate::ui::components::ScenarioInstrumentsWritebackState>,
)
```

### Step 2: `buffer.original_path` セット後に追記するブロック

`event.layout.scenario` (既に `Option<serde_json::Value>` として `SidecarLayout` に含まれる) から直接 seed する。

```rust
// --- Issue B fix: seed registry/metadata from cache JSON ---
if let Some(sc) = &event.layout.scenario {
    let has_ref = sc.get("instruments_ref").is_some();
    let ids: Vec<String> = sc
        .get("instruments")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    registry.replace_all(&ids);
    registry.editable = !has_ref;

    *scenario = crate::ui::components::ScenarioMetadata {
        schema_version: sc.get("schema_version").and_then(|v| v.as_u64()).map(|n| n as u32),
        instruments: ids,
        start: sc.get("start").and_then(|v| v.as_str()).map(String::from),
        end: sc.get("end").and_then(|v| v.as_str()).map(String::from),
        granularity: sc.get("granularity").and_then(|v| v.as_str()).map(String::from),
        initial_cash: sc.get("initial_cash").and_then(|v| v.as_i64()),
    };

    // parse_scenario_system が original .json を読み直して上書きしないよう
    // watch state を先回りで埋める
    watch.last_path = buffer.original_path.clone();
    watch.last_mtime = buffer.original_path.as_ref()
        .map(|p| p.with_extension("json"))
        .and_then(|jp| std::fs::metadata(&jp).ok())
        .and_then(|m| m.modified().ok());

    // ファイル由来の代入なので writeback loop を起こさない
    writeback.revision = writeback.flushed_revision;
}
```

### 挿入位置

既存コードの中で `buffer.original_path = ...` / `buffer.cache_path = ...` をセットした直後、
`allocator.bump_to_at_least(...)` の前。

## 変更しないもの

- `parse_scenario_system` — 変更なし（起動時は watch state が一致するので no-op になる）
- `sync_registry_from_scenario_loaded_system` — 変更なし
- `SidecarLayout` / `CacheRestoreRequested` — 変更なし（`scenario` フィールドは既存）

## 検証方法

1. `cargo check` / `cargo test --lib` が通ること
2. 手動: `app_state.json` の `scenario.instruments` を `["1301.TSE"]` に固定
3. アプリ起動 → Sidebar が `1301.TSE` 1 件のみ表示されること（`7203.TSE` が出ない）
4. `parse_scenario_system` が watch state 一致で skip されることをログで確認（`SCENARIO parsed from sidecar` が起動時に出ないこと）

---

# Plan: Refactor — save/load 周りのシンプル化

## 動機

Issue B fix で「起動時は `app_state.json` だけ使う」方針が確定した。  
一方、現在の save/load 実装には以下の複雑さが残っている。

| 問題 | 箇所 |
|---|---|
| **保存トリガーが 3 系統**（Ctrl+S / ウィンドウ閉じ / debounced）あり、うち Explicit save だけ `build_layout_for_explicit_save`（registry sync 付き）を呼ぶ。他 2 系統は `build_layout`（sync なし）を呼ぶ差異がある | `layout_persistence.rs` |
| **Scenario writeback が 2 系統**：`flush_sidecars_now`（Explicit save 前）と `writeback_scenario_instruments_system`（毎 tick dirty check）が同じ cache JSON を書く。同フレームで両方走ると競合 | `layout_persistence.rs` |
| **`sync_to_cache` / `copy_sidecar_to_cache`** が stale 削除 → コピーの順序依存を持ち、`handle_save_layout_system` 内の pre-flush と実行順が暗黙的に結合している | `layout_persistence.rs` |

## 方針

> **「保存は常に同じ 1 関数を通す。読み込みは 2 系統（startup / user-open）だが contract を明示する。」**

## Step 1: `build_layout` を統一する

`build_layout_for_explicit_save` を廃止し、`build_layout` 内で常に registry sync を行う。

```rust
// 変更前
fn build_layout_for_explicit_save(...) -> SidecarLayout { /* registry sync あり */ }
fn build_layout(...)                  -> SidecarLayout { /* registry sync なし */ }

// 変更後
fn build_layout(...) -> SidecarLayout { /* 常に registry sync */ }
```

影響: `handle_save_layout_system` の呼び出しを `build_layout_for_explicit_save` → `build_layout` に切り替えるだけ。

## Step 2: 保存の共通ヘルパーを 1 つに束ねる

3 系統が共通で行う「build → write cache JSON」を `save_to_cache(...)` として抽出。  
各 system はこれを呼ぶだけにする。

```rust
fn save_to_cache(
    layout: SidecarLayout,
    paths: &CacheStatePaths,  // cache_state_paths() の結果
) -> std::io::Result<()> {
    save_layout_to(&layout, &paths.json)?;
    // 必要なら .py 書き出しもここに集約
    Ok(())
}
```

## Step 3: Scenario writeback を 1 系統に絞る

`flush_sidecars_now`（Explicit save 前に呼ばれる手動フラッシュ）を **廃止**し、  
`writeback_scenario_instruments_system`（毎 tick dirty check）に一本化する。

- Explicit save 時は「保存前に必ず dirty を解消したい」だけなので、  
  代わりに `writeback_scenario_instruments_system` を **`PostUpdate` より前の専用ステージ**に昇格させ、同フレームで確実に flush されることを保証する。
- 同フレーム競合は消滅する。

## Step 4: `sync_to_cache` の順序依存を除去する

`copy_sidecar_to_cache` の stale 削除ロジックを `sync_to_cache` 内部に閉じ込め、  
呼び出し元が「コピー前に削除したか」を意識しなくて済むようにする。

```rust
// 変更後: sync_to_cache が完結している
fn sync_to_cache(original: &Path, cache: &Path) -> std::io::Result<()> {
    if cache.exists() { std::fs::remove_file(cache)?; }
    std::fs::copy(original, cache)?;
    Ok(())
}
```

## 変更しないもの

- 2 系統のロードパス（`apply_cache_restore_system` / `apply_layout_system`）— 役割が異なるため維持
- `CacheRestoreRequested` / `LayoutLoadRequested` イベント構造
- `SidecarLayout` 構造体

## 検証方法

1. `cargo check` / `cargo test --lib` が通ること
2. Ctrl+S → cache JSON が更新されること
3. ウィンドウ閉じ → cache JSON が更新されること
4. 1 秒 debounce → cache JSON が更新されること
5. instruments を変更 → `writeback_scenario_instruments_system` が 1 回だけ flush すること（ログ確認）
6. Issue B の検証 1〜4 が引き続き通ること

## 実装順序

Issue B fix（上の Plan）を先にマージしてから本 Refactor を行う。  
理由: refactor 中に Issue B の watch state ロジックが混在すると diff が読みにくいため。
