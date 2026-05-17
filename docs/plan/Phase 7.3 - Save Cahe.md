# Save 以外は Cache のみへ書く計画

## Summary
- 通常操作による変更、特に Chart × / Instruments 行削除 / Run 直前同期 / window close / layout autosave は、`%LocalAppData%\the-trader-was-replaced\app_state.json` と `app_state.py` のみ更新する。
- ユーザーが明示的に `Save (Ctrl+S)` または `Save As` した時だけ、元の `.py` / `.json` へ反映する。
- Run はユーザー選択どおり **cache のみ**を最新化して実行し、元ファイルは触らない。

## 現状の挙動（修正前）
- `flush_sidecars_now(ids, original_py, cache_sidecar)` は両方の `Option<&Path>` が `Some` の場合 **元 sidecar と cache sidecar の両方** に `scenario.instruments` を書く（[components.rs:646-685](src/ui/components.rs#L646-L685)）。
- `ScenarioMetadata` を更新する経路は `parse_scenario_system` のみで、これは `buffer.original_path.with_extension("json")` の mtime 変化を watch している（[scenario_parser.rs:42-58](src/ui/scenario_parser.rs#L42-L58)）。Run config に詰める `scenario.instruments` はこの resource 経由（[menu_bar.rs:587](src/ui/menu_bar.rs#L587)）。
- `build_layout(..., preserve_scenario_from)` を呼ぶ 4 か所のうち **3 か所が原本 `.json` を読み**、1 か所だけ cache を読んでいる（非対称）:
  - `handle_save_layout_system`: `buffer.original_path` → 原本 ([layout_persistence.rs:387](src/ui/layout_persistence.rs#L387))
  - `handle_save_as_layout_system`: `cache_json` → cache ([layout_persistence.rs:465](src/ui/layout_persistence.rs#L465))
  - `save_layout_on_window_close`: `buffer.original_path` → 原本 ([layout_persistence.rs:901](src/ui/layout_persistence.rs#L901))
  - `debounced_autosave_system`: `buffer.original_path` → 原本 ([layout_persistence.rs:943](src/ui/layout_persistence.rs#L943))

## Key Changes

### KC1. writeback 呼び出し側で cache のみを指定する
- `flush_sidecars_now` の **シグネチャは変えない**（`original_py: Option<&Path>` と `cache_sidecar: Option<&Path>` を受ける現 API のまま）。CacheOnly は呼び出し側で `original_py=None` を渡すことで表現する。
  - 採用理由: enum 追加は API 表面が増えるだけで、`Option` 2 引数ですでに表現可能。誤呼び出し対策として、CacheOnly 呼び出し点には `original_py=None` の意図コメントを置き、テストで「元 sidecar 不変」を固定する。実装後レビューでは `rg "flush_sidecars_now"` で全 callsite を再確認する。
- 変更対象（いずれも既存と同様に `paths.cache_sidecar.as_deref()` を渡す。`paths.cache_sidecar == None` のときは `flush_sidecars_now` が `Err("no writeback target")` を返すので、既存の `last_error` 設定 / `error! + continue` でそのまま扱う）:
  - `writeback_scenario_instruments_system`: `original_py=None`, `cache_sidecar=paths.cache_sidecar.as_deref()` で呼ぶ ([components.rs:619-623](src/ui/components.rs#L619-L623))。
  - `handle_strategy_run_system` の inline flush: 同上 ([menu_bar.rs:576-580](src/ui/menu_bar.rs#L576-L580))。
- `flush_sidecars_now` の戻り値 `Option<SystemTime>` は元 sidecar の新 mtime を返す契約のため、CacheOnly 呼び出しでは常に `Ok(None)` となる。`ScenarioFileWatchState.last_mtime` は更新されない（**KC2 と整合**: 原本を触らないので watch 抑止も不要）。

### KC2. registry 編集を `ScenarioMetadata` に直接反映する
- **これが本計画の要**。CacheOnly に切ると「原本 sidecar mtime 変化 → reparse → `ScenarioMetadata` 更新」の連鎖が切れる。`Run` config が古い `instruments` を送ってしまうのを防ぐため、registry 変更を `ScenarioMetadata.instruments` に **ファイル経由ではなく resource 直接**で同期する。
- 追加: `sync_scenario_metadata_from_registry_system`（仮称）
  - 条件: `registry.editable && writeback.revision != writeback.flushed_revision`
    - `registry.is_changed()` ではなく既存 `writeback_scenario_instruments_system` と同じ revision dirty 判定にする。これにより `writeback_scenario_instruments_system` と挙動が完全に同期する（fire するときは両方 fire、しないときは両方しない）。
    - ファイルロード時の挙動: `sync_registry_from_scenario_loaded_system` が `revision = flushed_revision` にリセットするが、続く `mark_registry_dirty_system` が `registry.is_changed()` を観測して `revision += 1` するため、結果として **KC2 と writeback は 1 回 fire する**。ただし書き込み内容はロード直後と同値なので副作用なし（既存 writeback の挙動も同様で、これは新規問題ではない）。
    - ユーザー編集（Chart × / 行削除）も同じ経路で fire する。
  - 動作: `scenario.instruments != registry.as_slice()` のときだけ `scenario.instruments = registry.as_slice().to_vec()` を行う。同値なら no-op にし、cache writeback 失敗などで dirty が残った場合でも `ScenarioMetadata` の change detection を毎 tick 汚さない。
  - 配置: KC2 と `writeback_scenario_instruments_system` はどちらも同じゲート条件で起動する姉妹システム。chain 内で KC2 → writeback の順に並べる（KC7 参照）。`handle_strategy_run_system` の ordering 制約は **KC7 を参照**（`.after(sync_scenario_metadata_from_registry_system)` を必須付与）。
- 実装場所: 既存 registry / writeback system と同じ `src/ui/components.rs` に `pub fn` として追加し、`src/ui/mod.rs` の `use crate::ui::components::{...}` に import する。テストからも同じ関数を直接使う。
- 副作用: KC2 自身は `ScenarioLoadedFromFile` を発火しない（このイベントは `parse_scenario_system` 専用）。registry 編集はファイルロードと別経路で扱う、という意図的な分離。
- `instruments_ref` シナリオ: `registry.editable == false` のときはこのシステムは何もしない → 既存挙動を維持。

### KC3. cache 永続化系の `preserve_scenario_from` を cache_json に切り替える
- 通常操作（Save 以外）は cache JSON の scenario を保持し、原本の古い scenario で上書きしないようにする。
- 変更対象 (2 箇所):
  - `save_layout_on_window_close` [layout_persistence.rs:901](src/ui/layout_persistence.rs#L901)
  - `debounced_autosave_system` [layout_persistence.rs:943](src/ui/layout_persistence.rs#L943)
- 参照する cache JSON path は **`paths.cache_sidecar.as_deref()`**（`ScenarioWritebackPaths` resource 経由）で取得し、KC1 / KC4 と経路を統一する。production では `ScenarioWritebackPaths.cache_sidecar = cache_state_paths().0` で初期化される（[mod.rs:97-98](src/ui/mod.rs#L97-L98)）ため挙動同等。テストでは resource を直接差し替えられる利点もある。`None` の場合は **`preserve_scenario_from=None`** にフォールバックし、原本の古い scenario は読まない。これにより cache autosave / window close が stale 原本で cache scenario を巻き戻す事故を防ぐ。
  - `save_layout_on_window_close` / `debounced_autosave_system` には `paths: Res<ScenarioWritebackPaths>` を追加する。`cache_state_paths()` は保存先 cache JSON の取得にだけ残してよいが、`preserve_scenario_from` は必ず `paths.cache_sidecar.as_deref()` を使う。
  - 既存 `handle_save_as_layout_system` が `cache_state_paths()` を直呼びしている箇所も `paths` resource 経由に揃える小修正を同 PR に含める。

### KC4. 明示 Save 系は cache の健全性に依存させず原本へ正しく保存する
- `handle_save_layout_system` / `handle_save_as_layout_system` の冒頭で、`registry.editable == true` なら `flush_sidecars_now(registry.as_slice(), None, paths.cache_sidecar.as_deref())` を呼ぶ（**cache 側だけ最新化**。原本はこの時点では触らない）。
  - 失敗時は `warn!` 以上でログを出すが、**Save 自体は継続**する。理由: cache JSON が存在しない / 壊れている / LocalAppData 側だけ一時的に書けない場合でも、ユーザーが明示した Save は原本 `.py/.json` を更新できるべき。ここで `continue` すると「cache が壊れたせいで Ctrl+S できない」という Medium リスクになる。
  - `registry.editable == false`（`instruments_ref` シナリオ）のときはこの pre-flush をスキップし、cache JSON 既存 scenario をそのまま使う。
- 明示 Save 用に小さな helper を追加する（仮称 `build_layout_for_explicit_save`）。中身は既存 `build_layout` を呼んだ後、必要なら `layout.scenario` を in-memory で補正する:
  1. scenario の preserve source は `cache_sidecar` を第一候補にする。
  2. cache sidecar が無い / 読めない場合、明示 Save に限って `old_original_path.with_extension("json")` を fallback source にする。通常 autosave / window close ではこの fallback を使わない。
  3. `registry.editable == true` の場合、preserve した scenario の `instruments` を `registry.as_slice()` で必ず上書きする。scenario object が無い場合は `ScenarioMetadata` から最小 v2 scenario object を作る。ただし `start` / `end` / `granularity` / `initial_cash` のいずれかが欠けている場合は、壊れた scenario を作らず `error!` してその Save イベントを skip する（backend の `scenario.validate` はこれらを required として扱うため）。
  4. `registry.editable == false` の場合は `instruments_ref` などの既存 scenario 形状を壊さないため、scenario を in-memory で変更しない。
- `save_layout_to(json_path, ...)` で原本 `.json` に scenario 含めて書き出す。
- 既存の `sync_to_cache(&py_path)` 呼び出しで cache と原本を一致させる（[menu_bar.rs:617-635](src/ui/menu_bar.rs#L617-L635)）。
  - `sync_to_cache` 成功時だけ fragment dirty / `strategy_auto_save.dirty` を clear する。加えて `buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py)` を必ず再設定する。これは前回の同期失敗で `buffer.cache_path=None` に落とした後、次回 Save 成功で Run 可能状態へ戻すために必要。
  - `sync_to_cache` 失敗時は原本 Save 成功を取り消さないが、cache は最新ではないため dirty 状態を維持し、`buffer.cache_path = None` に落として Run をブロックする（footer は既存の `cache_path None` disable 表現を使える）。これにより「Ctrl+S 後に古い cache.py で Run される」事故を防ぐ。
- `handle_save_as_layout_system` は buffer を新パスへ更新する前に `old_original_path` を保持し、上記 fallback source として使う。これにより Save As 先の新規 `.json` がまだ存在しない状態でも、cache または旧原本から scenario を回収できる。
- 追加 system param: `handle_save_layout_system` / `handle_save_as_layout_system` に `registry: Res<InstrumentRegistry>`, `scenario: Res<ScenarioMetadata>`, `paths: Res<ScenarioWritebackPaths>` を注入。

### KC5. `flush_sidecars_now` の `original_py: None` 時 mtime セマンティクス
- 現状の戻り値ドキュメント（`Ok(None) = cache のみ書いた or 元 sidecar が無い`）は CacheOnly でも valid。コメントを更新するのみで挙動変更なし。

### KC6. Save 後・外部編集後の終端性
- `parse_scenario_system` の mtime watch 対象は **原本 `.json` のみ**（[scenario_parser.rs:51](src/ui/scenario_parser.rs#L51)）で、cache `.json` は監視対象外。
- Save 後の正常系の流れ:
  1. Save handler が原本 `.json` を書く（mtime=T1）。`sync_to_cache` で cache も T1。
  2. 次 tick: `parse_scenario_system` が T1 を読み reparse + `watch.last_mtime ← T1`。
  3. chain 内で `sync_registry_from_scenario_loaded_system` → `mark_registry_dirty_system` → KC2 & writeback (CacheOnly) が走り、cache `.json` を上書き（mtime=T2）。原本は触らない。
  4. 次 tick: 原本 mtime は T1 のままで `watch.last_mtime == T1` なので reparse はスキップ。終端。
- 外部編集（ユーザーが原本 `.json` を別エディタで保存）の流れ: 原本 mtime が変わる → reparse → 上記 3 → 終端。
- `sync_to_cache` 失敗時の流れ: 原本 `.py/.json` は保存済み、cache は未同期、`buffer.cache_path=None` で Run 不可。次回 Save / Save As / Open の cache 同期成功時に `buffer.cache_path` を固定 cache `.py` へ再設定して通常状態へ戻る。
- 結論: ループしない。正常系では registry / `ScenarioMetadata` / 原本 / cache の 4 者は Save 直後に内容一致状態に収束する。cache 同期失敗系では「原本保存済み・Run 不可」の明示的な中間状態に止める。

### KC7. UiPlugin system chain と Run handler ordering（必須）
- 既存 [mod.rs:120-136](src/ui/mod.rs#L120-L136) の `.chain()` に KC2 のシステムを差し込む。順序:
  ```
  (
      parse_scenario_system,
      sync_registry_from_scenario_loaded_system,
      mark_registry_dirty_system,
      sync_scenario_metadata_from_registry_system,   // ← KC2 追加 (writeback の前)
      writeback_scenario_instruments_system,
      instrument_chart_sync_system,
  ).chain()
  ```
- **必須**: `handle_strategy_run_system` に `.after(sync_scenario_metadata_from_registry_system)` を付与する。
  - 理由: Run handler は `scenario.instruments` を読んで `RunStrategy` config に詰める（[menu_bar.rs:587](src/ui/menu_bar.rs#L587)）。KC2 が `scenario.instruments` を書く。同 tick で「registry 編集 → Run クリック」が同フレームに来た場合、Run handler が KC2 の前に走ると stale な instruments が backend に渡る。`.after()` で順序を保証する。
  - `footer_pause_resume_system.before(handle_strategy_run_system)` の既存 ordering（[mod.rs:120](src/ui/mod.rs#L120)）と両立する。

## Test Plan

### 既存テスト更新が必要なモジュール
- `writeback_scenario_instruments_tests`（[components.rs:753-1075](src/ui/components.rs#L753-L1075)）:
  - 「元 sidecar に instruments が書き込まれる」assert を「**元 sidecar は不変** / cache sidecar のみ更新」に反転。
  - 元 sidecar mtime 観測前提のテスト（あれば）を撤去し、代わりに `ScenarioFileWatchState.last_mtime` が `None` のままであることを assert。
- `sync_registry_from_scenario_loaded_tests` / `mark_registry_dirty_tests`（[components.rs:469-588](src/ui/components.rs#L469-L588)）:
  - KC2 のシステムを追加した chain で、ファイルロード → 1 tick 後 `ScenarioMetadata.instruments == ロードしたファイルの値` であることを 1 ケース追加（KC2 が same-content の上書き fire を起こすが結果整合性は崩れないことの確認）。
- run-flow E2E（[components.rs:1597-1660](src/ui/components.rs#L1597-L1660)）:
  - registry 編集 → Run → `TransportCommand::RunStrategy` の `config.instruments` が **registry と一致**（KC2 の効果検証）。
  - 元 sidecar が編集前のままであることを assert。
- inline-flush テスト（[components.rs:1162-1408](src/ui/components.rs#L1162-L1408)）:
  - Run 直前 flush が cache のみ更新するように assert を更新。

### 新規テスト（KC2）
- `sync_scenario_metadata_from_registry_system` 単体（system 単独構成、`mark_registry_dirty_system` は含めない）:
  - `writeback.revision` を手動で `flushed_revision + 1` にセット → 1 tick → `ScenarioMetadata.instruments == registry.as_slice()`。
  - `registry.editable == false` 時は no-op。
  - `revision == flushed_revision` のときは no-op（dirty ゲート確認）。
  - `ScenarioMetadata.instruments` がすでに registry と同値の場合は no-op（change detection を汚さない）。

### 新規テスト（KC4 Save 系）
- `handle_save_layout_system`: registry 編集 → Ctrl+S → 元 `.json` の `scenario.instruments` が registry と一致 / `sync_to_cache` 後 cache と元が一致。
- `flush_sidecars_now` が失敗した場合（cache path 不在 = `paths.cache_sidecar == None`、または cache JSON missing）でも Ctrl+S は継続し、元 `.json` の `scenario.instruments` が registry と一致すること。あわせて `sync_to_cache` 失敗時も原本保存を取り消さないことを確認。
- `sync_to_cache` 失敗時は fragment dirty / `strategy_auto_save.dirty` が維持され、`buffer.cache_path == None` になり、Run ボタン経路が送信しないこと。続く Save 成功時は dirty が clear され、`buffer.cache_path` が固定 cache `.py` に復旧すること。
- cache JSON が壊れている場合、明示 Save は旧原本 sidecar を fallback source として使い、`scenario.instruments` だけ registry で上書きして保存できること。
- cache / 旧原本のどちらからも scenario を回収できず、`ScenarioMetadata` の required fields が欠けている場合は、明示 Save が壊れた scenario を書かずに skip されること。
- `handle_save_as_layout_system`: 既存挙動（cache scenario を新 path に取り込み）を回帰テストで固定化、加えて cache が無い場合は `old_original_path` 側から scenario を回収し、`registry.editable == true` なら instruments が registry に置換されることを確認。
- `instruments_ref` (`registry.editable == false`) の Save / Save As では `instruments_ref` 形状を維持し、`scenario.instruments` への変換や writeback を行わないこと。

### 回帰テスト
- `save_layout_on_window_close` / `debounced_autosave_system`:
  - 元 `.py/.json` が変わらない。
  - cache 側の最新 `scenario.instruments` が古い元 JSON の内容で上書きされない（KC3 の cache→cache preserve 検証）。
- `save_layout_on_window_close` / `debounced_autosave_system` で cache JSON が無い場合は、原本 sidecar を fallback として読まないこと（stale 原本からの巻き戻し防止）。
- `instruments_ref` シナリオで cache/元どちらにも writeback しない（既存契約維持）。

### 実行確認
- `cargo test writeback`
- `cargo test scenario`
- `cargo test chart`
- `cargo test layout`
- `cargo test run_flow` （E2E 統合 [components.rs:1597-1660](src/ui/components.rs#L1597-L1660)）
- `rg "flush_sidecars_now"` で callsite を確認し、通常 writeback / Run は `original_py=None`、明示 Save は helper 経由で原本 `.json` を保存する構造になっていることを確認。

## Assumptions
- 「Save Ctrl+S」の例外には、ユーザー明示保存である `Save As` も含める。
- Run は元ファイルを保存せず、cache `.py/.json` の最新状態で実行する。
- 元ファイルを変更する操作は `Save` / `Save As` のみとし、アプリ終了・ウィンドウ移動・Chart close は cache 永続化に限定する。
- `ScenarioMetadata.instruments` の更新は今後 (a) ファイルロード = `parse_scenario_system` と (b) registry 編集 = `sync_scenario_metadata_from_registry_system` の二経路を持つ。両者の system order は `(a) → (b)` で固定し、registry 側が優先される（編集中の値がファイル読み戻しに勝つ）。

## Out of Scope
- `ScenarioMetadata` の他フィールド（`start` / `end` / `granularity` / `initial_cash`）の registry 由来更新。これらは現状 sidecar JSON 由来のみで、UI からの編集経路を持たないため本計画では触らない。
- 複数 strategy ファイルを跨ぐ cache 戦略。`StrategyBuffer.original_path` 1 本に紐づく単一 cache を前提とする。
