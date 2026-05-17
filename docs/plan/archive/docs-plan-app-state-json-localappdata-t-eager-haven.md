# Plan: scenario 読み込み先を `ScenarioReadTarget` に集約してファイル open/save/cache の責務を単純化する

## Context

現状 `parse_scenario_system` ([src/ui/scenario_parser.rs:43-160](src/ui/scenario_parser.rs#L43-L160)) は `buffer.original_path.with_extension("json")` を **暗黙に** sidecar 読み込み先として導出している。これは「`buffer.original_path` を設定した人が誰であろうと、scenario truth は `<original>.json`」という強い前提を埋め込んでおり、3 か所の loader (`apply_cache_restore_system` / `handle_strategy_file_load_system` / `apply_layout_system`) のうち、起動時の cache 復元だけがこの前提と合わない。なお `apply_layout_system` は自身では `buffer.original_path` を設定せず `StrategyFileLoadRequested` を fire して `handle_strategy_file_load_system` に委譲するため、§3b の対応でカバーされる:

- 起動時 `apply_cache_restore_system` ([src/ui/layout_persistence.rs:335-430](src/ui/layout_persistence.rs#L335-L430)) は cache の `app_state.py` から fragment を復元しつつ `buffer.original_path = event.layout.strategy_path` を設定する。
- 次フレームの `parse_scenario_system` は `<strategy_path>.json`（元 sidecar）を読み、cache 側の scenario を上書きしてしまう。

- `apply_cache_restore_system` に scenario seed 処理を増やす
- `ScenarioInstrumentsWritebackState.suppress_next_registry_dirty` flag を新設
- `ScenarioFileWatchState.last_path` / `last_mtime` を手で偽装して `parse_scenario_system` を no-op に追い込む

という方針で「ファイルを開く / 保存 / cache 読む / cache 書く」の各経路に分岐を 1 段ずつ足す band-aid。

代わりに、scenario の **読み込み先** を 1 つの Resource (`ScenarioReadTarget`) に集約し、各 loader が「次に読むべき sidecar JSON 」を 1 度だけ明示する形にすれば、

- cache 復元時は `ScenarioReadTarget = Some(cache_json)`
- user open 時は `ScenarioReadTarget = Some(original.json)`
- close 時は `None`

となり、起動上書きバグは自然に消える。`suppress_next_registry_dirty` も `watch state 偽装` も不要になり、責務境界が「loader が target を指す / parser は target を読むだけ」の 2 段に縮む。

## Goals

- `parse_scenario_system` を「`buffer.original_path` を覗き見て勝手に sidecar JSON を導出する」polling から「`ScenarioReadTarget: Option<PathBuf>` を読むだけ」に縮める。
- 起動 cache 復元では cache sidecar (`%LocalAppData%/the-trader-was-replaced/app_state.json`) を target に設定し、元 sidecar を起動の truth source にしない。
- user open / save-as / close の挙動は維持（target の指し先がそれぞれの場面で自然に書き換わる）。
- 元プランで予定していた `suppress_next_registry_dirty` / watch state 偽装は導入しない（不要になる）。
- 外部エディタによる「現在 active な sidecar」への編集検知は維持（mtime polling は target に対して継続）。

## Non-Goals

- `StrategyBuffer.original_path` の意味は変えない（ファイル名表示・save 先・status label 用途のまま）。
- `sync_to_cache` / `flush_sidecars_now` / `copy_sidecar_to_cache` の write ロジック（何をどのファイルに書くか）は今回は変えない（read 側の責務集約だけで起動バグが解ける）。ただし後述 §5 の mtime 更新は `writeback_scenario_instruments_system` の振る舞いに追加する。
- `SidecarLayout` JSON schema は変更しない。

## Approach

### 1. 新しい Resource: `ScenarioReadTarget`

[src/ui/components.rs](src/ui/components.rs) に追加:

```rust
/// `parse_scenario_system` が次に読むべき sidecar JSON のフルパス。
/// `None` のときは「読むものなし」= scenario を default にリセット。
/// 各 loader (cache 復元 / user open / close) が排他的にセットする。
#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioReadTarget(pub Option<std::path::PathBuf>);
```

`AppExt` / plugin 登録箇所に `.init_resource::<ScenarioReadTarget>()` を追加。

### 2. `parse_scenario_system` を target-driven に書き換え

[src/ui/scenario_parser.rs:43](src/ui/scenario_parser.rs#L43)

変更前後の差分の要点:

```rust
pub fn parse_scenario_system(
    target: Res<ScenarioReadTarget>,                 // ← NEW: buffer.original_path を見ない
    mut scenario: ResMut<ScenarioMetadata>,
    mut watch: ResMut<ScenarioFileWatchState>,
    mut loaded_events: EventWriter<ScenarioLoadedFromFile>,
    mut cleared_events: EventWriter<ScenarioClearedFromFile>,
) {
    let json_path: Option<PathBuf> = target.0.clone();
    let current_mtime = json_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    if watch.last_path == json_path && watch.last_mtime == current_mtime {
        return;
    }
    watch.last_path = json_path.clone();
    watch.last_mtime = current_mtime;

    let Some(json_path) = json_path else {
        cleared_events.send(ScenarioClearedFromFile { source_path: None });
        *scenario = ScenarioMetadata::default();
        return;
    };

    // 以下、JSON 読み込み・parse は現行ロジックそのまま
}
```

- 「`.py` 渡されて `.with_extension("json")`」の導出を捨てる。
- target を切り替えれば watch 自動でそちらへ移行（mtime ベースの外部編集検知は維持）。

### 3. 各 loader で `ScenarioReadTarget` をセット

#### 3a. 起動 cache 復元

[src/ui/layout_persistence.rs:335 `apply_cache_restore_system`](src/ui/layout_persistence.rs#L335)

`buffer.original_path` を設定する直後で、

```rust
let (cache_json, cache_py) = cache_state_paths()...;
// ...既存処理...
buffer.original_path = event.layout.strategy_path.as_ref().map(PathBuf::from);
buffer.cache_path = Some(cache_py.clone());

// NEW: 起動 truth source は cache sidecar。元 sidecar は読まない。
scenario_target.0 = Some(cache_json.clone());
```

引数に `mut scenario_target: ResMut<ScenarioReadTarget>` を追加。`cache_state_paths()` の戻り値の第 1 要素 (cache_json) を target に渡す。

#### 3b. user open / layout restore

[src/ui/menu_bar.rs:410 `handle_strategy_file_load_system`](src/ui/menu_bar.rs#L410)

`buffer.original_path = Some(event.path.clone())` の直後で:

```rust
let sidecar_path = event.path.with_extension("json");
scenario_target.0 = Some(sidecar_path.clone());
```

`buffer.original_path` セット（line 434）と `let sidecar_path` 算出（line 476）の間には ~40 行のコードがあるため単純な reorder ではない。`buffer.original_path = Some(event.path.clone())` の直後に `let sidecar_path = event.path.with_extension("json");` を新たに追加し、`ScenarioReadTarget` をセットしてから、既存 line 476 の重複束縛を削除する。引数に `mut scenario_target: ResMut<ScenarioReadTarget>` を追加。

#### 3c. close / 新規（path None）— ロールバック箇所

`buffer.original_path = None` にしている既存箇所と同じ場所で `scenario_target.0 = None` も実行する。

実装前に `buffer.original_path = None` を grep し、実在する箇所だけ対応すること。現時点では `handle_save_layout_system` の初回 Save ロールバック（`if was_new` ブロック）に 2 箇所のみ確認済み（`layout_persistence.rs` 内の 2 行）。`menu_bar.rs` のロード失敗パスには現在 None 代入が存在しないため、実装前 grep で確認できた場合のみ追加対応する。

#### 3d. Save 成功時・Save As 成功時の target 更新

`parse_scenario_system` が `buffer.original_path` を見なくなる変更後は、Save/Save As 成功時に `ScenarioReadTarget` を明示的に更新しないと旧 target または `None` を読み続ける。

**`handle_save_layout_system`（Save 全ケース — `was_new` 有無を問わない）**

`build_layout_for_explicit_save` を呼ぶ際、`cache_sidecar` 引数に `paths.cache_sidecar.as_deref()` ではなく **`scenario_target.0.as_deref()`** を渡す:

```rust
let layout = match build_layout_for_explicit_save(
    &panels,
    &camera,
    &*buffer,
    &registry,
    &scenario,
    scenario_target.0.as_deref(),  // ← 現行の paths.cache_sidecar.as_deref() を置き換え
    fallback_json.as_deref(),
) { ... }
```

**理由**: user open 後は `ScenarioReadTarget = Some(original.json)` であり、現在の truth source は original sidecar。ここで `cache_sidecar`（古い cache 由来）を優先すると、外部エディタで original.json の `start/end/granularity/initial_cash` を編集後に Save しても stale cache 側の値で上書きされる。`ScenarioReadTarget` を優先することで「現在 active な truth から preserve する」原則が一貫する。

- `scenario_target.0 = Some(cache_json)` の場合: 実質的に現行の `paths.cache_sidecar` 優先と同等 ✓
- `scenario_target.0 = Some(original.json)` の場合: original を優先 ✓
- `scenario_target.0 = None` の場合: None → `fallback_json.as_deref()` が候補 ✓

引数に `scenario_target: Res<ScenarioReadTarget>`（または `ResMut`）を追加。

`save_layout_to` が成功した直後（`sync_to_cache` 呼び出しの前）:
```rust
// NEW: Save 成功後は original sidecar を read target に切り替える。
// was_new = false かつ target が既に Some(json_path) の場合も冪等に実行してよい。
// was_new = false かつ target = cache_json（cache restore 後の初回 Save）の場合に必須。
scenario_target.0 = Some(json_path.clone());
```
`json_path` は保存先 sidecar（`buffer.original_path.with_extension("json")` または初回 Save ダイアログで選択されたパス）。`was_new = false` かつ `ScenarioReadTarget` が既に `Some(json_path)` だった場合は冪等で問題ない。

ロールバックが必要な箇所（`was_new = true` のみ）:

| 箇所 | 発生条件 | 追加するロールバック |
|------|----------|-------------------|
| `build_layout_for_explicit_save` が `None` 返し | scenario 必須フィールド欠落 | `buffer.original_path = None` / `buffer.cache_path = None` / `scenario_target.0 = None` |
| `save_layout_to` 失敗 | JSON 書き込みエラー | 同上 |
| `.py` 書き込み失敗（**新規**、line 560 付近） | `.py` ファイル書き込みエラー | `buffer.original_path = None` / `buffer.cache_path = None` / `scenario_target.0 = None`（JSON は書かれているが `.py` がないため file-open 状態を確立しない） |

> `.py` 失敗分岐（line 560）は現行コードが完全に error ログのみで rollback がない。この節での追加が本所見（**High**）の修正箇所。

**`handle_save_as_layout_system`**

`save_layout_to` が成功した直後（`bump_writeback_for_save_as` 呼び出しの前）で:
```rust
// NEW: Save As 確定後は新 sidecar を read target に切り替える
let old_scenario_target = scenario_target.0.clone();
scenario_target.0 = Some(json_path.clone());
```

ロールバック 3 箇所（`build_layout_for_explicit_save` が `None` 返し、`save_layout_to` 失敗、`.py` 保存失敗）では旧 target を復元する:
```rust
scenario_target.0 = old_scenario_target;
```
`old_scenario_target` は `save_layout_to` 呼び出し前に退避しておく。引数に `mut scenario_target: ResMut<ScenarioReadTarget>` を追加。

### 4. 旧結合を削除

- `parse_scenario_system` から `buffer: Res<StrategyBuffer>` 依存を撤去。
- `ScenarioFileWatchState.last_path` の doc comment を必ず更新する。型変更はないが **意味が変わる**（変更前: `.py` パス / 変更後: `.json` パス = `ScenarioReadTarget` の直前値）。更新を怠ると将来の読者が「なぜ JSON パスが入っているのか」を理解できなくなる。
- 元プランで予定していた `ScenarioInstrumentsWritebackState.suppress_next_registry_dirty` は導入しない。

### 5. registry writeback への影響と `watch.last_mtime` 更新

不変条件: 「`ScenarioReadTarget` が指すファイルへの書き込みが完了したとき、`ScenarioFileWatchState.last_mtime` はそのファイルの新 mtime と一致していなければならない」。この条件を満たすことで `parse_scenario_system` の誤検知を防ぐ。§5a は「起動時に書き込みが発生しないため mtime 更新は不要」を示し、§5b は「writeback 時に同一ファイルへ書いた後の mtime 更新」が必要な理由を示す。

#### 5a. 起動時（suppress flag 不要の理由）

`apply_cache_restore_system` → 次フレームで `parse_scenario_system` が cache JSON を読む → `ScenarioLoadedFromFile` が発火 → 既存の [`sync_registry_from_scenario_loaded_system`](src/ui/components.rs#L488) が `registry.replace_all(&ev.instruments)` と同 tick で `writeback.revision = writeback.flushed_revision` を実行する。

「ファイル由来の registry 代入は writeback dirty を起こさない」既存メカニズムにそのまま乗るだけで、suppress flag を追加せずに dirty 抑止が成立する。

#### 5b. `writeback_scenario_instruments_system` が同一ファイルを書く問題（要対処）

変更後、`ScenarioReadTarget = cache_json` の状態で `writeback_scenario_instruments_system` が cache_json に書いた場合:

- `flush_sidecars_now(registry.as_slice(), None, cache_json)` は `Ok(None)` を返す（原設計では original sidecar mtime のみ返す）
- `watch.last_mtime` が更新されない
- 次 tick の `parse_scenario_system` がキャッシュファイルの mtime 変化を検知 → 再 parse → `ScenarioLoadedFromFile` → `sync_registry.replace_all` → `mark_registry_dirty_system` が `registry.is_changed()` を検知して `revision++` → writeback が再実行 → **無限ループ**

**修正**: `writeback_scenario_instruments_system` で cache_sidecar への書き込み成功後、cache ファイルの mtime を読み返して `watch.last_mtime` を更新する。ただし **`ScenarioReadTarget` が cache_sidecar を指しているときのみ** 転記する。user open 後は `ScenarioReadTarget = original sidecar` なので cache mtime を転記しない（`last_path=original` / `last_mtime=cache` という不整合を防ぐため）:

```rust
match flush_sidecars_now(registry.as_slice(), None, paths.cache_sidecar.as_deref()) {
    Ok(_new_mtime) => {
        writeback.flushed_revision = writeback.revision;
        writeback.last_error = None;
        // NEW: target が cache sidecar のときのみ mtime を転記して parse_scenario_system の誤検知を防ぐ。
        // target が original sidecar の場合は cache mtime を入れないこと
        // （watch.last_path=original だが last_mtime=cache になり次 tick で再 parse が起きる）。
        if target.0.as_deref() == paths.cache_sidecar.as_deref() {
            if let Some(cache_path) = paths.cache_sidecar.as_deref() {
                if let Ok(meta) = std::fs::metadata(cache_path) {
                    if let Ok(mtime) = meta.modified() {
                        watch.last_mtime = Some(mtime);
                    }
                }
            }
        }
    }
    ...
}
```

この変更で "writeback → parse 再発火 → writeback → ..." の無限ループを防ぐ。`flush_sidecars_now` の戻り値型は変えない（`Ok(None)` のまま）。`writeback_scenario_instruments_system` の引数に `target: Res<ScenarioReadTarget>` を追加する（`paths` / `watch` はすでに存在する）。

#### 5c. 他の cache_json 書き込み経路と parser の関係（対処不要だが要理解）

`ScenarioReadTarget = cache_json` のとき、以下の経路も `cache_json` の mtime を更新する:

| 経路 | 発火条件 | 書き込み内容 |
|------|----------|------------|
| `handle_strategy_run_system` inline flush（line 580） | Run ボタン押下 | scenario JSON のみ |
| Save pre-flush（line 458） | 明示 Save イベント | scenario JSON のみ |
| Save As pre-flush（line 589） | 明示 Save As イベント | scenario JSON のみ |
| `save_layout_on_window_close`（line 1090） | ウィンドウ閉じ | full layout |
| `debounced_autosave_system`（line 1133） | 1 秒デバウンス | full layout |

これらが mtime を変化させると次 tick の `parse_scenario_system` が再発火するが、**無限ループにはならない**。収束チェーン（Run inline flush を例に取る）:

1. Run inline flush が `cache_json` を書く → mtime 変化
2. 次 tick: `parse_scenario_system` が mtime 変化を検知 → 再 parse → `ScenarioLoadedFromFile`
3. `sync_registry_from_scenario_loaded_system`: `registry.replace_all` + **`writeback.revision = writeback.flushed_revision`** をリセット
4. `mark_registry_dirty_system`: `registry.is_changed()` = true（`replace_all` の DerefMut 経由）→ `revision++`
5. `writeback_scenario_instruments_system`: `revision > flushed_revision` → `cache_json` に再書き込み → §5b の mtime 同期（`watch.last_mtime` 更新）
6. 次 tick: `watch.last_mtime == current_mtime` → early return ✓

`cache_json` 書き込み 1 回につき「1 回余分な parse + 1 回余分な writeback」が発生するが 2 tick 以内に収束する。Run inline flush は §5b の mtime 同期経由で必ず収束するため追加対処は不要。

§3d/§3e により **明示 Save / Save As 成功後** は `ScenarioReadTarget` が original_sidecar に切り替わるため、その後の autosave は parser を再発火しなくなる。起動から最初の明示 Save まで（= target が cache_json のままの期間）が唯一の余分な write 発生区間であり、これは許容範囲とする。

**新たな実装変更は不要**（§5b で充足）。この節は将来の読者がデバッグ時に混乱しないための説明であり、設計上の抜け穴ではない。

### 6. 起動シーケンスのフレーム保証

`apply_cache_restore_system` (Startup) → `parse_scenario_system` (Update) → `sync_registry_from_scenario_loaded_system` (Update) の system 順序は、現状すでに `apply_cache_restore_system` → `parse_scenario_system` の順で観察される（元バグの発症経路と同じ）。また `src/ui/mod.rs` では `parse_scenario_system` → `sync_registry_from_scenario_loaded_system` → `sync_registry_from_scenario_cleared_system` → `mark_registry_dirty_system` → `sync_scenario_metadata_from_registry_system` → `writeback_scenario_instruments_system` が `.chain()` 済み。追加 chain は不要。

frame-0 での誤発火については: `ScenarioReadTarget` は `Default` が `None` のため、起動直後の frame 0 では `watch.last_path == None` かつ `json_path == None` → equality check が `true` → 早期 return。`ScenarioClearedFromFile` は発火しない。

### 7. 単純化ステップ（本体に組み込む）

主目的の read target 集約と同一 PR 内で整理する。

#### 7a. `build_layout` の `preserve_scenario_from` を JSON path として扱う（必須）

`build_layout`（[src/ui/layout_persistence.rs:175](src/ui/layout_persistence.rs#L175)）の引数 `preserve_scenario_from: Option<&Path>` は内部で `.with_extension("json")` を実行するが、実際の呼び出し側（`build_layout_for_explicit_save`、`apply_cache_restore_system`）はすでに `.json` パスを渡しているため二重変換になっている。本番 caller の直接呼び出し 4 箇所はすべて `.json` パスを渡しており修正不要。テスト（[src/ui/layout_persistence.rs:1382](src/ui/layout_persistence.rs#L1382)）が `.py` パスを渡している唯一の例外であり、テスト側を `.json` に修正する。

変更内容:
1. `build_layout` の引数名を `preserve_scenario_from` → `preserve_scenario_json` にリネーム。
2. 関数内の `.map(|p| p.with_extension("json"))` を削除し、`preserve_scenario_json` をそのまま使う。
3. doc comment（line 158-160）を「`preserve_scenario_json` に `.json` パスを渡すと…」に書き直す。
4. テスト（line 1382 周辺）で `py_path` を渡していた箇所を `py_path.with_extension("json")` に修正。

#### 7b. `sidecar_has_windows` helper を切り出す（必須）

[src/ui/menu_bar.rs:481-487](src/ui/menu_bar.rs#L481-L487) の 7 行 `read_json_with_bom_strip + serde_json` チェーンを `layout_persistence.rs` に `pub(crate) fn sidecar_has_windows(path: &Path) -> bool` として抽出する。

- `path.exists()` チェックも helper 内に含める（`sidecar_exists &&` の前置き不要に）。
- 失敗時は現状通り `false` 扱い。
- schema は変更しない。
- `menu_bar.rs` 側の呼び出しは `sidecar_has_windows(&sidecar_path)` 1 行に置き換える。
- `menu_bar.rs` に `use crate::ui::layout_persistence::sidecar_has_windows;` を追加するか、完全修飾パスで呼ぶ。

#### 7c. `sync_to_cache` 戻り値変更（任意 — 差分が膨らむなら後回し）

現状は `handle_strategy_file_load_system` / `handle_save_layout_system` / `handle_save_as_layout_system` がそれぞれ `cache_state_paths()` を呼んで `buffer.cache_path` を設定し、その後に `sync_to_cache()` も内部で同じ `cache_state_paths()` を呼んでいる。

`sync_to_cache(original_py)` を `std::io::Result<(PathBuf, PathBuf)>`（`cache_json`, `cache_py`）にして、呼び出し側は成功時にその戻り値で `buffer.cache_path` と `ScenarioReadTarget` を更新する形にすると、cache path の再計算と「先に cache_path を入れてから sync 失敗で None に戻す」分岐を減らせる。

変更対象ファイル（実施する場合）:
- `src/ui/menu_bar.rs` — `sync_to_cache` 戻り値を `Result<(PathBuf, PathBuf)>` に変更、全呼び出し側を更新
- `src/ui/layout_persistence.rs` — 3 箇所の `sync_to_cache` 呼び出しと、直前の `cache_state_paths()` 呼び出しを統合

`floating_window.rs`（line 293）は `sync_to_cache` を呼ばず `cache_state_paths()` を直接呼んでいるため影響外。

`copy_sidecar_to_cache` の挙動（stale cache 削除）は変えない。

#### 7d. `buffer.original_path = None` 箇所の確認（実装前チェック）

実コードの grep では、`buffer.original_path = None` は `handle_save_layout_system` の初回 Save ロールバック 2 箇所のみ。`menu_bar.rs` のロード失敗パスは現在 `original_path` を None に戻していない。§3c の記述はこの grep 結果に基づく。実装時は再 grep で確認し、実在する箇所だけ `scenario_target.0 = None` を追加すればよい。別途「§7d に対応する実装ステップ」はなく、§3c の実装作業に含まれる。

## Files To Change

- [src/ui/components.rs](src/ui/components.rs)
  - `ScenarioReadTarget` 追加と `init_resource`。
  - `writeback_scenario_instruments_system`: 引数に `target: Res<ScenarioReadTarget>` を追加。`target == cache_sidecar` の場合のみ `watch.last_mtime` を cache ファイルの実 mtime で更新（§5b 参照）。
- [src/ui/scenario_parser.rs](src/ui/scenario_parser.rs)
  - `parse_scenario_system` の引数差し替えと本体短縮。
  - 既存テスト（buffer.original_path を立てて発火させているもの）を target Resource をセットする形に書き換え。
- [src/ui/layout_persistence.rs](src/ui/layout_persistence.rs)
  - `apply_cache_restore_system` で `ScenarioReadTarget` を cache_json にセット（§3a）。
  - `handle_save_layout_system`: `build_layout_for_explicit_save` の `cache_sidecar` 引数を `scenario_target.0.as_deref()` に変更（§3d/Medium2）。`save_layout_to` **成功**後（`was_new` 有無を問わず）に `scenario_target.0 = Some(json_path)` を追加（§3d）。`was_new = true` の失敗ロールバック 3 箇所（build_layout None 返し・save_layout_to 失敗・.py 書き込み失敗）は `scenario_target.0 = None`（§3c/§3d）。
  - `handle_save_as_layout_system` の Save As **成功**後に `scenario_target.0 = Some(json_path)` を追加、ロールバック 3 箇所は `old_scenario_target` を復元（§3d）。引数に `mut scenario_target: ResMut<ScenarioReadTarget>` を追加。
  - `build_layout` の引数 `preserve_scenario_from` → `preserve_scenario_json` にリネームし、内部の `.with_extension("json")` を削除。doc comment も更新（§7a）。
  - `sidecar_has_windows(path: &Path) -> bool` helper を追加（§7b）。
- [src/ui/menu_bar.rs](src/ui/menu_bar.rs)
  - `handle_strategy_file_load_system` で `ScenarioReadTarget` を original sidecar にセット（§3b）、同関数内の `sidecar_has_windows` 呼び出し置き換え（§7b）は同一 edit pass で行う。
  - close / path None 経路でも `None` にリセット（実装前 grep で該当箇所が存在する場合のみ、§3c 参照）。
  - `use crate::ui::layout_persistence::sidecar_has_windows;` を追加（または完全修飾パスで呼ぶ）。
- [src/ui/mod.rs](src/ui/mod.rs)
  - `.init_resource::<ScenarioReadTarget>()` を追加。`parse_scenario_system` 周辺の `.chain()` は現状維持（§6 参照）。

`flush_sidecars_now` / `copy_sidecar_to_cache` の write ロジックは変えない。`sync_to_cache` の変更は §7c（任意）として分離されており、今 PR では変えない（実施する場合は `src/ui/menu_bar.rs` と `src/ui/layout_persistence.rs` の呼び出し側 3 箇所が追加対象）。

## Test Plan

### 新規 unit / ECS テスト

1. `parse_scenario_uses_target_path_not_buffer_original`
   - `buffer.original_path = Some("/foo.py")` でも `ScenarioReadTarget = Some(cache_json)` なら cache JSON が読まれる。
2. `parse_scenario_returns_to_default_when_target_none`
   - 事前に `ScenarioReadTarget.0 = Some(path)` + `watch.last_path = Some(path)` + `watch.last_mtime = Some(mtime)` をセットして 1 tick 実行（読み込み済み状態にする）。その後 `ScenarioReadTarget.0 = None` に切り替えて次 tick を実行。→ `ScenarioMetadata::default()` + `ScenarioClearedFromFile`。`None` スタートのまま回すと `None == None` の equality check で early return するため、必ず「Some から None へ切り替え」の構成にすること。
3. `cache_restore_points_scenario_target_to_cache_json`
   - `CacheRestoreRequested` を流したあと `ScenarioReadTarget == cache_json`。
4. `cache_restore_then_parse_does_not_overwrite_with_original_sidecar`
   - tmp に `<strategy>.json` = `["7203.TSE"]`、cache JSON = `["1301.TSE"]` を置く。
   - 1 update 後、`ScenarioMetadata.instruments == ["1301.TSE"]`。
5. `cache_restore_does_not_dirty_writeback`
   - 4 の構成で `writeback.revision == writeback.flushed_revision` のまま。
6. `user_open_points_scenario_target_to_original_sidecar`
   - `StrategyFileLoadRequested { path: "/foo.py" }` 後、`ScenarioReadTarget == Some("/foo.json")`。
7. `external_edit_of_active_sidecar_is_redetected`
   - target = cache_json で 1 回読んだ後、cache_json の mtime を変えて update → `ScenarioMetadata` 再 parse される。
8. `sidecar_has_windows_returns_false_for_nonexistent`（`src/ui/layout_persistence.rs`）
   - 存在しないパスを渡すと `false`。
9. `sidecar_has_windows_returns_true_for_valid_windows_json`（`src/ui/layout_persistence.rs`）
   - tmp に `{"windows": [{}]}` を書いて渡すと `true`。`{"scenario": {...}}` のみで `windows` キーが無い場合は `false`。
10. `save_success_updates_scenario_read_target_from_cache_to_sidecar`（`src/ui/layout_persistence.rs`）
    - `ScenarioReadTarget = Some(cache_json)`（cache restore 後の状態）、`buffer.original_path = Some(py_path)`（was_new = false）でセットアップ。
    - `SaveLayoutRequested` を fire して 1 tick 実行。
    - `ScenarioReadTarget == Some(py_path.with_extension("json"))` を assert。
    - これにより §3d の「`was_new = false` + cache-restored 後の Save で target が正しく切り替わる」を自動検証する。
11. `save_as_success_updates_scenario_read_target_to_new_sidecar`（`src/ui/layout_persistence.rs`）
    - `ScenarioReadTarget = Some(old_sidecar)`、`SaveAsLayoutRequested` を fire → 成功パス（モック `FileDialog` または tmp ファイルへの direct 呼び出し）。
    - `ScenarioReadTarget == Some(new_sidecar)` を assert。
    - Note: `FileDialog` がテスト環境で動作しない場合は `save_as_layout_to` 相当のロジックを direct 呼び出しに書き換えるか、system ではなくヘルパー関数レベルでテストする。
12. `save_failure_rollback_preserves_old_target`（`src/ui/layout_persistence.rs`）
    - `ScenarioReadTarget = Some(old_sidecar)`、`SaveLayoutRequested` を fire → `save_layout_to` が失敗するように tmp ディレクトリを読み取り専用にするか Err を返す mock を用意。
    - `ScenarioReadTarget == Some(old_sidecar)` のまま（target が変わっていない）ことを assert。
13. `cache_write_while_target_is_cache_json_terminates_within_two_ticks`（`src/ui/components.rs` または `scenario_parser.rs`）
    - `ScenarioReadTarget = Some(cache_json)`、`watch.last_mtime` = 古い mtime で起動。
    - tick 1: cache_json の mtime を更新（autosave シミュレート）→ parse 再発火を確認。
    - tick 2: `watch.last_mtime` が更新されていること + parse が **再発火しない**（早期 return）ことを確認。
    - これにより §5b の mtime 同期が autosave 経路でも機能することを保証する（§5c の回帰テスト）。

### 既存テストの修正

- `parse_scenario_system` 系テストで `buffer.original_path` を立てている箇所は、`app.init_resource::<ScenarioReadTarget>()` を追加したうえで `ScenarioReadTarget.0 = Some(path.with_extension("json"))` を立てる形に置き換える。対象関数（`src/ui/scenario_parser.rs` 内、7 関数）:
  - `test_system_parses_instruments_from_sidecar`
  - `test_system_normalizes_v1_single_instrument`
  - `test_system_resets_when_sidecar_missing`
  - `test_system_emits_loaded_event_on_first_read`
  - `test_system_does_not_reemit_when_mtime_unchanged`
  - `test_system_detects_instruments_ref_key`
  - `test_editable_resets_to_true_when_switching_to_sidecar_without_scenario`
- `test_writeback_does_not_retrigger_scenario_reload`（`src/ui/components.rs`）: 現在は `buffer.original_path = cache_py` で原 sidecar を指す形で構成されているが、変更後は `parse_scenario_system` が `ScenarioReadTarget` を見るため、`ScenarioReadTarget.0 = Some(cache_json)` を立てる形に書き換える。更新しないと "writeback 後に parse が再発火しない" の検証が空振りになり §5b の回帰を見落とす。
- §7a の `build_layout` リネーム対応: `src/ui/layout_persistence.rs` のテスト（line 1382 周辺）で `py_path` を `preserve_scenario_from` に渡していた箇所を `py_path.with_extension("json")` に変更する（helper が `.with_extension` を呼ばなくなるため、テストは自分で JSON パスを渡す必要がある）。

### Manual Verification

1. `%LocalAppData%/the-trader-was-replaced/app_state.json` の `scenario.instruments` を `["1301.TSE"]` にする。
2. 元 sidecar `<strategy>.json` の `scenario.instruments` を `["7203.TSE"]` にする。
3. アプリ起動 → sidebar / chart が `1301.TSE` のみ。
4. 起動ログに `SCENARIO parsed from sidecar` が cache_json を指して出る（元 sidecar ではない）。
5. 起動だけでは cache JSON の mtime が動かない（不要な writeback 無し）。
6. Sidebar で instrument 追加削除 → cache sidecar のみに writeback（元 sidecar は変更されない）。`watch.last_mtime` が更新されていれば次 tick の parse が再発火しないことをログで確認（SCENARIO parsed ログが余分に出ない）。
7. メニューから元 sidecar を user-open → 以降は `<strategy>.json` 内容が反映され、外部エディタで `<strategy>.json` を書き換えると数秒以内に UI に反映される。また、この状態で Sidebar を編集しても `SCENARIO parsed ログ` が余分に出ないこと（cache mtime を誤転記しない）。
8. 新規状態から初回 Save → `ScenarioReadTarget` が新 sidecar を指す（ログで確認）。Sidebar 編集後に parse ログが余分に出ない。
9. 任意のファイルを開いた状態で Save As → `ScenarioReadTarget` が新 sidecar に切り替わる。Save As をキャンセルまたは失敗させると旧 target が維持される。
10. 起動直後（Save 前）に 1〜2 秒待ち、debounced autosave が発火するような変更を行う。`SCENARIO parsed` ログが 1 回余分に出て止まること（ループしないこと）を確認。Save 後は autosave が起きても parse ログが余分に出ないこと（target が original sidecar に切り替わっているため）。
11. `cargo check` および `cargo test --lib ui::` (該当モジュール) が通る。

## Acceptance Criteria

- `parse_scenario_system` が `buffer.original_path` に依存しない。
- 起動時の scenario truth source は `app_state.json`（cache sidecar）。
- 元 sidecar は user open / layout restore / 明示 Save 成功 時に `ScenarioReadTarget` の指し先になる（cache restore 直後の「起動後まだ Save していない状態」では target = cache_json のまま）。
- 起動由来の seed は `ScenarioInstrumentsWritebackState.revision` を増やさない（既存 `sync_registry_from_scenario_loaded_system` 経由で自然成立）。
- `suppress_next_registry_dirty` flag や watch state 偽装は導入されていない。
- ユーザー操作由来の registry edit は従来通り writeback dirty を作る。
- writeback が cache_json に書いた後、`ScenarioFileWatchState.last_mtime` が cache_json の新 mtime で更新されており、次 tick の `parse_scenario_system` が再発火しない。ただし `ScenarioReadTarget` が original sidecar のとき（user open 後）は cache mtime を `last_mtime` に転記しない。
- Save 成功後（`was_new` 有無を問わず）は `ScenarioReadTarget` が保存先 sidecar を指す。Save As 成功後も同様。失敗ロールバック後は旧 target に戻る（`was_new = true` の Save は `None`、Save As は保存前の target）。
- autosave（debounced / window close）や pre-flush が `cache_json` を書いても、既存の §5b mtime 同期により 2 tick 以内に収束し、無限ループを起こさない。明示 Save 成功後は target が original sidecar に切り替わるため、その後の autosave では parse が再発火しない。
- 既存の user-open / save / save-as / writeback / close の挙動に regression なし。

## Implementation Log

### Completed (2026-05-18)

#### §1 — `ScenarioReadTarget` Resource 追加
- ✅ `src/ui/components.rs` に `ScenarioReadTarget` struct と `init_resource` を追加（13 箇所）
- ✅ `src/ui/mod.rs` に `.init_resource::<ScenarioReadTarget>()` を追加

#### §2 — `parse_scenario_system` を target-driven に書き換え
- ✅ `src/ui/scenario_parser.rs` を `ScenarioReadTarget` 参照に変更（7 箇所の既存テスト修正を含む）

#### §3 — 各 loader で `ScenarioReadTarget` をセット
- ✅ `src/ui/layout_persistence.rs`: `apply_cache_restore_system` / `handle_save_layout_system` に target セット追加
- ✅ `src/ui/menu_bar.rs`: `handle_strategy_file_load_system` に target セット追加

#### §7b/7c — リファクタ
- ✅ `build_layout` 引数 `preserve_scenario_from` → `preserve_scenario_json` にリネーム（二重 `.with_extension("json")` を解消）
- ✅ `sidecar_has_windows(path: &Path) -> bool` helper を `layout_persistence.rs` に切り出し（`menu_bar.rs` の 7 行チェーンを 1 行に縮小）

### 知見

#### `mod.rs` の `pub use` 注意点
`src/ui/mod.rs` の `pub use components::{...}` は明示列挙形式のため、`components.rs` に新しい pub struct を追加しても自動でエクスポートされない。`ScenarioReadTarget` を追加した際は `pub use components::{..., ScenarioReadTarget, ...}` に手動で追加が必要だった。

#### テスト `App` への `init_resource` 追加が必要
ECS テスト（`App::new()` で構築するテスト）では `DefaultPlugins` を入れないため、`parse_scenario_system` が `Res<ScenarioReadTarget>` を要求すると Resource not found でパニックする。`ScenarioFileWatchState` の `init_resource` と同じ場所に `init_resource::<ScenarioReadTarget>()` を追加する必要がある。今回修正: `scenario_parser.rs` 7 箇所、`components.rs` 13 箇所、`layout_persistence.rs` 7 箇所。

### 検証結果
- `cargo test --lib`: **155 passed / 0 failed**
