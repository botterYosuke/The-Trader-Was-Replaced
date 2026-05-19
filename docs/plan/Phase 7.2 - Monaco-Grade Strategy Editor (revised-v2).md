# Strategy Editor を Monaco-grade に格上げする (Zed 参照) — revised-v2

> v2 改訂理由: revised 版に対するレビュー (重大 8 点) を反映。Phase A 初手で詰む致命的指摘 (Parser !Sync / root-child Query 不一致 / set_redraw 明示) と、実装後発覚バグになる残り 5 点を本文・Caveat・Verification に取り込んだ。設計の骨子 (5 モジュール分割 / dirty 二重利用回避 / attrs 専用更新 / 重ね順) は revised 版から不変。

## Context

`src/ui/strategy_editor.rs` (1254 行) は現状、bevy_cosmic_edit ベースの**単色テキストエディタ**にすぎない。基本機能 (undo/redo / 自動保存 / フラグメント管理 / multi-spawn / レイアウト永続化) は完成しているが、**ソースコードエディタとしての視認性・編集体験**が欠けている — syntax highlight も行番号もスクロールバーも Find も無い。Tab は cosmic_edit の input ハンドラが `Key::Character` でない logical key を無視するため何も起きない (= 入りも focus 遷移もしない、無音で吸われる)。

ユーザは「Zed を参考に高級ソースコードエディタに変えたい」。スコープは **Monaco-grade** で合意済 (LSP・診断・command palette・マルチカーソルは含めず、syntax / gutter / scrollbar / find&replace / auto-indent / Tab→spaces / bracket match まで)。Syntax highlight は **tree-sitter-python** ベースで合意。

`.claude/skills/zed/SKILL.md` に「Monaco-grade Strategy Editor は実装済」と書かれているが、これは 0.15 移行時に**実態は剥がれている** (今回の作業で実装し直す)。Phase 7.2 完了時にスキル本文も同期する。

## アーキテクチャ概要

`strategy_editor.rs` を肥大化させず、**5 つの新規モジュール**に責務を分割。既存の `StrategyBuffer` (Resource、`source` は持たない — `original_path` / `cache_path` のみ) と `StrategyFragment` (Component、`source: String` + `dirty: bool`) を**唯一の source of truth** として、各モジュールは「source が変わったら派生表示を再計算」する設計。

### 既存 ECS 構造 (重要・revised-v2 で明示)

現行 `spawn_strategy_editor_panel` ([src/ui/strategy_editor.rs:103–190](src/ui/strategy_editor.rs)) は **2 entity 構成**:

- **root entity** (`WindowRoot` マーカー)
  - `StrategyFragment { source, dirty }` — テキストの source of truth
  - `StrategyEditorId { region_key }` — multi-spawn 個体識別
  - `PanelKind::StrategyEditor`
  - title bar / 枠などの floating window 装飾
- **child editor entity** (`StrategyEditorContent` マーカー、root の content_area の子)
  - `CosmicEditBuffer` — unfocused 時に描画される実バッファ
  - `Option<CosmicEditor>` — focus を得たときに付く編集中バッファ
  - `StrategyEditorId { region_key }` — root と同じ region_key を貼って join 可能にしてある
  - `TextEdit2d` / `Sprite` / `CosmicBackgroundColor` / `CosmicTextAlign` 等

→ **`StrategyFragment` と `CosmicEditBuffer` は別 entity**。1 つの `Query` で両方取ることはできない。本プラン内のすべての highlight / sync 系 system は、既存 sync 群 ([line 234, 283 など](src/ui/strategy_editor.rs)) と同じ「root を `With<WindowRoot>` で取る Query + editor を `With<StrategyEditorContent>` で取る Query、`StrategyEditorId.region_key` でジョイン」の 2 段構成にする。

### `StrategyFragment.dirty` の二重利用回避 (重要)

既存コードでは `fragment.dirty = true` は「autosave が拾うべき変更がある」を意味する (`debounced_strategy_autosave_system` が見て書き出し後 false に戻す)。Highlight 再計算のトリガに同じフラグを流用すると「autosave が先に dirty を落とすと highlight が走らない」競合になる。

→ **highlight 系は Bevy 標準の `Changed<StrategyFragment>` (フィルタ) を使う**。`dirty: bool` フィールドは autosave 専用のまま据え置く。

### 責務分割と Zed 参照対応

| 新規モジュール (`src/ui/`) | 責務 | Zed 参考 (`.claude/skills/zed/src/`) |
|---|---|---|
| `strategy_editor_highlight.rs` | tree-sitter-python で AST 取得 → highlight query で span 列を生成 → `BufferLine::set_attrs_list` で各行の attrs **だけ**を差し替え (テキストは触らない = カーソル不変)、最後に Buffer / Editor の `set_redraw(true)` を明示 | `crates/syntax_theme/src/syntax_theme.rs` (capture→color マップ), `crates/language/src/syntax_map.rs` (snapshot 戦略) |
| `strategy_editor_gutter.rs` | エディタ左に独立 cosmic_text `Buffer` をもう 1 つ持つ Sprite を配置、行番号文字列を同じ Metrics で描画 | `crates/editor/src/element.rs` (paint_gutter / layout_gutter) |
| `strategy_editor_scrollbar.rs` | エディタ右に `Sprite` で thumb を描画。Pointer<Drag> で scroll を変更 | `crates/editor/src/scroll.rs` (ScrollAnchor + offset), `scroll/autoscroll.rs` (strategy enum) |
| `strategy_editor_input.rs` | Tab → 4 spaces, Enter → 前行インデント継承, 括弧キーで自動閉じ | `crates/editor/src/editor.rs::tab`, `editor::newline`, `crates/language/src/language.rs::indent_size_for_line` |
| `strategy_editor_find.rs` | 世界空間の小型パネル (`Sprite` + `CosmicEditBuffer` × 2 = query / replacement) を `spawn_floating_window` ヘルパで配置、マッチ行の attrs を上書き、Enter/F3 で次へ。**Find editor には専用マーカー `FindQueryEditor` / `FindReplacementEditor` を付け、`StrategyEditorContent` は絶対に付けない** | `crates/search/src/buffer_search.rs` (BufferSearchBar の query/replacement editor 分離) |

`bracket_match` (現在カーソル位置の対応括弧ハイライト) は `strategy_editor_highlight.rs` 内に小さな関数として同居 (Zed の `highlight_matching_bracket.rs` を参考、innermost bracket スキャンを cursor 周辺だけ)。

### attrs 専用更新で「カーソルリセット問題」を回避 (Critical)

`CosmicEditBuffer::set_rich_text` は内部で `Buffer::set_rich_text` を呼び、buffer.lines を**全部作り直す**ため `editor.action(Action::Click ...)` を別途呼ばないとカーソルが (0,0) にリセットされる。これは既存 `strategy_editor.rs:228` のコメントが既に警告している既知の foot-gun。

→ **テキストは触らず attrs だけ差し替える**。具体的には:

1. cosmic_text `Buffer::lines: Vec<BufferLine>` を `with_buffer_mut(|b| b.lines.iter_mut())` で借り、
2. 各 `BufferLine::set_attrs_list(AttrsList)` を呼んで line ごとの spans を更新、
3. **更新が終わったら `buffer.set_redraw(true)` を明示** (重要 — revised-v2 追加)。`set_attrs_list` 内部は `reset_shaping` を呼ぶが、これは字形再計算のフラグであり、render system が見る Buffer 全体の `redraw` flag は立たない。[crates/bevy_cosmic_edit/src/buffer.rs:142, 160, 215, 246–256](crates/bevy_cosmic_edit/src/buffer.rs) を見ても、CosmicEditBuffer の text 系 API は全て `set_redraw(true)` を明示的に呼んでおり、`Added<CosmicEditBuffer>` 用 system まで用意されている。
4. editor 側が付いていれば `editor.set_redraw(true)` も同様に呼ぶ。

これにより:
- buffer の line 構造は不変 → カーソル位置・選択範囲は保たれる
- 表示色のみ変わる → 1 フレームで再描画される

**`set_rich_text` を使うのは初回 spawn 時のみ** (`with_rich_text` で空 spans を渡し、初期色付けは `Changed<StrategyFragment>` の起動で 1 フレーム後に流す)。

### システム実行順序 (極めて重要 — revised-v2 で完全固定)

cosmic_text の `AttrsList` は **後から上書きしたものが勝つ**。下の順で `after()` を**全段連結**する:

```
1. sync_strategy_buffer_to_editor_system   (既存・UndoRedoApplied 駆動で set_text)
2. sync_editor_to_strategy_buffer_system   (既存・CosmicTextChanged 駆動で fragment.source 更新)
3. apply_tree_sitter_highlight             (Changed<StrategyFragment> 駆動 — 全行 attrs 差し替え)
4. apply_find_match_highlight              (FindReplaceState 駆動 — マッチ範囲の attrs 上書き)
5. apply_bracket_match_highlight           (cursor 位置駆動 — 2 文字だけ attrs 上書き)
```

なぜ 1, 2 を先に置くか:
- `sync_strategy_buffer_to_editor_system:262` は `set_text(..., Attrs::new())` で**全行 attrs をデフォルト色にリセット**する (Undo/Redo 直後のみ)。highlight を先に流すと set_text で色が消えて 1 フレーム白く点滅する。
- 既存 sync が走った後に同じフレーム内で highlight が再適用されることを保証する。

各システムは `BufferLine::set_attrs_list` で前段の AttrsList を**置き換える**のではなく、前段の結果をベースに `AttrsList::add_span` で重ね塗りする。逆順だと find マッチ色や bracket 色が syntax 色で消える (zed スキル Caveat 6)。

### Changed フィルタ駆動

- highlight 再計算は `Changed<StrategyFragment>` (Bevy フィルタ) で発火
- `sync_editor_to_strategy_buffer_system` (既存 286 行付近) と Undo/Redo (`PendingStrategySnapshotRestore` 経路、`editor_history.rs:359`) の**両方**で `fragment.source` を書き換えれば自動的に Changed が立つ (明示的な dirty 立てとは別経路)
- 入力中の毎フレーム再ハイライトはコスト測定後に判断: tree-sitter Python の incremental parse は 1KB 程度のソースで数 ms。300 行超で重い場合のみ 100ms デバウンス (`Local<Timer>`) を後付け

### cosmic_edit との接続点

- 初回 `with_rich_text(empty)` は **CosmicEditBuffer のビルダ呼び出し**で十分 (`spawn_strategy_editor_panel` 内)
- 以降の attrs 更新はヘルパ `fn for_each_buffer(entity, |buffer: &mut cosmic_text::Buffer| { ... })` に集約する。実装は:
  - `CosmicEditor` がエンティティに付いていれば `editor.with_buffer_mut(|b| f(b))` を呼ぶ (focused = render はこちらを見る、`render.rs:88` 参照)
  - `CosmicEditBuffer` 単独なら `f(&mut buffer.0)` を呼ぶ (unfocused = render はこちらを見る)
  - 両方付いている場合は editor 側のみで足りる (CosmicEditBuffer は editor がフォーカスを失ったときの復元元になっていて、focus 切替の瞬間に editor から書き戻される設計なので、焦らず editor だけ更新する)
- 実装簡略化のため Phase A v1 では **editor 側があれば editor のみ、無ければ CosmicEditBuffer 側のみ** を更新する (両更新は不要、上記の理由でロスしない)
- ヘルパ末尾で必ず `buffer.set_redraw(true)` ＋ editor 側があれば `editor.set_redraw(true)` を呼ぶ

## 実装フェーズ (5 段階、各 1 PR 想定)

依存関係: A → B,C,D,E は並行可能。Find (E) が最も独立。Tab/Enter (C) は input fork に触る可能性あり。

### Phase A: tree-sitter syntax highlight (最重要)

**Cargo.toml に追加:**
```toml
tree-sitter = "0.24"
tree-sitter-python = "0.23"
```

⚠️ **ABI 整合の罠**: `tree_sitter::Language` は ABI バージョン (`LANGUAGE_VERSION`) でランタイムチェックされる。`tree-sitter 0.24` は ABI 13〜14 をサポート、`tree-sitter-python 0.23.x` は ABI 14 で生成されている (2024 末時点)。**作業前に `cargo check` で `Parser::set_language` の戻り値型と `tree_sitter_python::LANGUAGE` (定数) vs `tree_sitter_python::language()` (関数) のどちらが正かを確認** — crate の README で最新 API を見る。プランは以下のどちらかに統一:

- 新 API: `parser.set_language(&tree_sitter_python::LANGUAGE.into())?;` (LanguageFn → Language 変換)
- 旧 API: `parser.set_language(&tree_sitter_python::language())?;`

Windows ビルドは `cc` クレートで自動 (vendored)、別途 toolchain 不要。

**highlights.scm の取得:**

`tree-sitter-python` (Rust crate) は `queries/highlights.scm` を `src/` 配下には**同梱していない**ことが多い (npm package には入っているが Rust crate は `node-types.json` と `parser.c` のみのケースあり)。プランは以下のいずれかを採用:

- (推奨) `.claude/skills/zed/src/crates/languages/src/python/highlights.scm` をリポジトリ内の `assets/queries/python_highlights.scm` にコピーして `include_str!("../../assets/queries/python_highlights.scm")` で読む — ライセンス確認 (Zed crates は GPL-3.0 / Apache-2.0 だが grammar query は通常 MIT or Apache、コピー時にライセンスヘッダを確認) + 出典コメント必須
- `tree-sitter-python` の `Cargo.toml::include` フィールドを確認して `queries/highlights.scm` が同梱されていれば `concat!(env!("CARGO_MANIFEST_DIR"), "/../tree-sitter-python/queries/highlights.scm")` のような workspace 内 path で参照 (ただし依存 crate の内部パスに依存するため脆い)

⚠️ **scm のコメントは `;`** (revised-v2 修正): tree-sitter query は Lisp/Scheme 系シンタックスで、行コメントは `;`。`// Source: ...` のような C/Rust 風コメントを `.scm` 冒頭に貼ると `Query::new` パース時に `Invalid syntax` で落ちる。**出典コメントの正しい形式:**

```scheme
; Source: zed-industries/zed crates/languages/src/python/highlights.scm
; Upstream license: see zed LICENSE (Apache-2.0 / GPL-3.0 dual)
```

**新規 `src/ui/strategy_editor_highlight.rs`:**

`SyntaxHighlighter` の格納形式 (revised-v2 修正 — 重大1):

`tree_sitter::Parser` は内部に `*mut TSParser` を持ち `unsafe impl Send for Parser` のみで **`!Sync`**。Bevy 0.15 の `Resource` トレイトは `Send + Sync + 'static` を要求するため、`Parser` を含む型を素の `Resource` にすると `the trait Sync is not implemented for tree_sitter::Parser` でコンパイルが通らない。

→ **`NonSend` リソースとして登録**する。

```rust
pub struct SyntaxHighlighter {
    pub parser: tree_sitter::Parser,
    pub query: tree_sitter::Query,
    pub capture_name_to_color: HashMap<u32, cosmic_text::Color>,  // capture index → 色
}

// startup_system:
fn init_syntax_highlighter(world: &mut World) {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_python::LANGUAGE.into();  // ABI 確認後に最終形に
    parser.set_language(&language).expect("tree-sitter-python ABI mismatch");
    let query_src = include_str!("../../assets/queries/python_highlights.scm");
    let query = tree_sitter::Query::new(&language, query_src).expect("invalid highlights.scm");
    let capture_name_to_color = build_capture_color_map(&query);
    world.insert_non_send_resource(SyntaxHighlighter { parser, query, capture_name_to_color });
}
```

system 側では `NonSendMut<SyntaxHighlighter>` で取り出す:

```rust
fn apply_tree_sitter_highlight_system(
    mut highlighter: NonSendMut<SyntaxHighlighter>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), (With<WindowRoot>, Changed<StrategyFragment>)>,
    mut editor_q: Query<(&StrategyEditorId, &mut CosmicEditBuffer, Option<&mut CosmicEditor>), With<StrategyEditorContent>>,
    mut prev_trees: Local<HashMap<Entity, tree_sitter::Tree>>,
) { ... }
```

(`NonSend*` は当該 system がメインスレッドでしか走らないことを Bevy に伝える。highlight は毎フレーム走らないので体感影響は無視できる。)

代替案: `Mutex<Parser>` でラップして通常 `Resource` にする手もあるが、Bevy は単一スレッドで Resource を独占するので Mutex の存在自体が無駄 (= NonSend の方が素直)。

`SyntaxTreeCache` の扱い (revised-v2 補強):

複数 editor で `previous_tree` を保持するのは `Local<HashMap<Entity, tree_sitter::Tree>>` で十分。`tree_sitter::Tree` は `Send + Sync` (内部に `Arc` を持つ。Parser と違って Tree は共有可)、Resource にも入れられるが Local の方が system 単位でカプセル化されて綺麗。

editor despawn 時の purge: `Local<HashMap>` 採用なら observer は使えない (Local は world から見えない) ので、system 冒頭で:

```rust
prev_trees.retain(|entity, _| editor_q.contains(*entity));
```

を呼ぶ (1 行)。Resource にする場合は計画書 v1 通り `OnRemove, StrategyEditorContent` observer で purge。

`HighlightTheme` の扱い:

capture 名 → `cosmic_text::Color` のマップは、起動時に `Query` をコンパイルした直後に capture index → color の `HashMap<u32, Color>` を作って `SyntaxHighlighter` に同居させる (上記コード参照)。実行時に `query.capture_names()` を毎回引かないため。**色は新規追加**: `src/ui/components.rs` に Dracula 風の `SYNTAX_KEYWORD` / `SYNTAX_STRING` / `SYNTAX_COMMENT` / `SYNTAX_FUNCTION` / `SYNTAX_TYPE` / `SYNTAX_NUMBER` / `SYNTAX_OPERATOR` を追加 (既存の panel/button 色とは独立の syntax 名前空間)。

`apply_tree_sitter_highlight_system` (revised-v2 修正 — 重大2 / 重大3 / 重大6):

```rust
fn apply_tree_sitter_highlight_system(
    mut highlighter: NonSendMut<SyntaxHighlighter>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), (With<WindowRoot>, Changed<StrategyFragment>)>,
    mut editor_q: Query<
        (Entity, &StrategyEditorId, &mut CosmicEditBuffer, Option<&mut CosmicEditor>),
        With<StrategyEditorContent>,
    >,
    mut prev_trees: Local<HashMap<Entity, tree_sitter::Tree>>,
) {
    // 1) editor despawn の掃除
    prev_trees.retain(|entity, _| editor_q.contains(*entity));

    // 2) Changed が立った root を走査
    for (frag_id, fragment) in &fragments_q {
        // 2a) 対応 editor を region_key で見つける
        let Some((editor_entity, _, mut buffer, editor_opt)) = editor_q
            .iter_mut()
            .find(|(_, ed_id, _, _)| ed_id.region_key == frag_id.region_key)
        else { continue };

        // 2b) Phase A v1: フル再パース (incremental は v2)
        //     incremental に上げる場合は CosmicTextChanged から InputEdit を組み立てて
        //     prev_tree.edit(&input_edit) を呼んでから parse(source, Some(&prev_tree)) する
        let new_tree = highlighter.parser.parse(&fragment.source, None);
        let Some(new_tree) = new_tree else { continue };

        // 2c) Query 実行 → 行ごとに AttrsList を組み立て
        let attrs_lists = build_attrs_lists_per_line(&fragment.source, &new_tree, &highlighter);

        // 2d) attrs を適用 (editor 側があれば editor のみ、なければ buffer 側)
        apply_attrs(&mut buffer, editor_opt.as_deref_mut(), &attrs_lists);

        // 2e) ★ redraw を明示 (重要)
        if let Some(editor) = editor_opt {
            editor.with_buffer_mut(|b| b.set_redraw(true));
            editor.set_redraw(true);
        } else {
            buffer.set_redraw(true);
        }

        // 2f) 次回の incremental 用に Tree を保存 (v2 で使用)
        prev_trees.insert(editor_entity, new_tree);
    }
}
```

**Phase A v1 と v2 の境界 (revised-v2 明示 — 重大6):**

- **v1**: `parser.parse(source, None)` で**フル再パース**。`prev_trees` は保持はするが使わない (v2 への布石)。Python 1KB のソースで数 ms 程度。300 行超で性能問題が出たら v2 へ。
- **v2** (Phase A 完了後の追加 PR): `CosmicTextChanged` から `(start_byte, old_end_byte, new_end_byte, start_position, old_end_position, new_end_position)` を組み立てて `prev_tree.edit(&InputEdit { .. })` → `parser.parse(new_source, Some(&prev_tree))` で incremental。CosmicTextChanged は新全文を持つだけで diff 情報は持たないので、`sync_editor_to_strategy_buffer_system` で `fragment.source` を書き換える直前に diff を計算して別 Event (例: `StrategyFragmentEdited { entity, input_edit }`) で配信する設計拡張が必要。

v1 で計画上の「incremental」を謳わず、シンプルなフル再パースとして実装する。

**Bracket match (highlight モジュール同居) — revised-v2 修正 (重大7):**

`apply_bracket_match_highlight_system` — **毎フレーム実行** (`Local<HashMap<Entity, cosmic_text::Cursor>>` で前回値を持ち、現在の `editor.cursor()` (cosmic_text Editor の `Deref` 経由) と比較して変化なしならスキップ)。`CosmicTextChanged` だけでは不十分な理由: cosmic_edit のイベントは `is_edit = true` の時にしか飛ばない (input.rs:518)、つまり矢印キー/クリックのみのカーソル移動では飛ばない。focused editor を `Query<(Entity, &CosmicEditor), With<StrategyEditorContent>>` で取り、`FocusedWidget.0` と一致したものだけ処理。

- カーソル位置周辺で `(){}[]` の対応をスキャン (innermost、AST 不要、最大 4096 文字くらいで打ち切り)
- マッチ 2 文字に `Attrs::color(BRACKET_MATCH_COLOR)` を **AttrsList::add_span で重ね塗り** (前段の syntax color の上)
- **前回ハイライト範囲の保持形式 (revised-v2 修正)**: bracket match は opener と closer の 2 箇所を同時にハイライトし、しばしば**別行にまたがる**。`Local<Option<(Entity, usize, Range<usize>)>>` (= 1 行 1 範囲) では不足する。以下の形を採用:

  ```rust
  /// editor entity → ハイライト中の (行, バイト範囲) のリスト (通常 2 要素: opener と closer)
  Local<HashMap<Entity, Vec<(usize, std::ops::Range<usize>)>>>
  ```

  cursor が動いた瞬間に各要素の range の attrs を syntax 由来に戻してから (= その行だけ再 highlight するか、対象 byte range にだけ syntax 色を上塗りする)、新しい opener / closer 2 箇所に bracket attrs を載せ、`HashMap` を更新する。最後に該当行の Buffer に `set_redraw(true)` を明示。

**修正: `src/ui/strategy_editor.rs`**
- 初期 `with_text` → `with_rich_text([("", default_attrs)], default_attrs)` に置換 (Phase A 時点では空 spans でも、`set_attrs_list` 経由更新が走る前提)
- `sync_strategy_buffer_to_editor_system:262` 周辺の `set_text` 経路は変更しない (set_text 後は Changed<StrategyFragment> が立つので別系統で再ハイライトされる)
- システム登録 (`src/main.rs` か Plugin 集約点) で **重要な順序連結 (revised-v2 明示)**:

  ```rust
  app.add_systems(
      Update,
      (
          sync_strategy_buffer_to_editor_system,
          sync_editor_to_strategy_buffer_system,
          apply_tree_sitter_highlight_system,
          apply_find_match_highlight_system,
          apply_bracket_match_highlight_system,
      ).chain()
  );
  ```

  `chain()` で前後依存を一括宣言。`add_systems` タプル 20 上限に注意 (Phase A〜E で 10+ system を追加するので、既存登録数次第で他のタプルから 1〜2 個外して別 `add_systems` 呼び出しに分割)。

### Phase B: 行番号ガター + スクロールバー

**新規 `src/ui/strategy_editor_gutter.rs`:**

- `LineNumberGutter` Component — エディタ左 36px (font_size 14 で 5 桁分 + padding) に `Sprite` (背景) + **もう 1 つの独立 `CosmicEditBuffer`** (read-only、`ReadOnly` component 付加) を子として配置
  - 別 cosmic_text Buffer を持つことで、Metrics をエディタと完全一致させ、行の高さズレを根本的に排除
  - 共通定数 `EDITOR_METRICS: Metrics = Metrics::new(14.0, 18.0)` を `strategy_editor.rs` に置き、ガター/エディタ/find 全部で共有
- `update_gutter_text_system` — `Changed<StrategyFragment>` で `(1..=line_count).map(|i| format!("{i:>4}")).join("\n")` を gutter buffer に `set_text`、最後に `set_redraw(true)` を明示
- スクロール追従: エディタ側の `editor.with_buffer(|b| { (b.scroll().line, b.scroll().vertical) })` を読み、ガター buffer の `set_scroll` に同じ値を入れる (line + vertical 両方コピー必須)
- **wrap モード (revised-v2 補強 — 重大5)**: エディタを `cosmic_text::Wrap::None` に固定する。`Buffer::set_wrap(&mut self, font_system: &mut FontSystem, wrap: Wrap)` は `FontSystem` 必須なので、startup 時に `Res<CosmicFontSystem>` から借りて `editor_buffer.0.set_wrap(&mut font_system.0, Wrap::None)` を 1 回呼ぶ (gutter 用の Buffer にも同様)。これで「source 行 == layout 行」になり、ガター行番号と scrollbar の line 数が一致する。長い行は横スクロール (cosmic_edit が `XOffset` で対応)。
  
  ⚠️ **widget 側の wrap 上書きに注意**: `TextEdit2d` の render system は `Sprite.custom_size` から buffer 幅を計算して `Buffer::set_size` 経由で wrap 値を**間接的に上書きするコードパス**を持つ場合がある (cosmic_edit のバージョンに依存)。Phase B 完了時の verification で「`Sprite.custom_size` を Phase B のレイアウト調整に合わせて変更した後でも、長い行が折り返されず横にはみ出すこと (= `Wrap::None` が維持されていること)」を目視確認する。維持されていない場合は `set_wrap(Wrap::None)` を毎フレーム呼ぶ system を 1 つ足す (1 回 / 1 editor の軽量呼び出し)。

**新規 `src/ui/strategy_editor_scrollbar.rs`:**

- `EditorScrollThumb { target_editor: Entity }` Component — エディタ右 8px に `Sprite`。`target_editor` フィールドで「どのエディタを操作する thumb か」を保持する (multi-spawn で複数 thumb が並ぶため必須)
- thumb サイズ: `thumb_h = (viewport_lines / total_lines).clamp(0.05, 1.0) * scrollbar_h`
- thumb 位置: `(scroll.line as f32 / (total_lines - viewport_lines).max(1) as f32) * (scrollbar_h - thumb_h)`
  - `Scroll::vertical` (line 内 pixel 微調整) は thumb 位置には反映しない (微小なので無視)
- `Pointer<Drag>` observer で thumb を縦ドラッグ → `trigger.target()` で thumb entity を取得 → そこから `EditorScrollThumb::target_editor` を引いて対象エディタを特定 → drag.delta.y を line に逆換算 → `editor.with_buffer_mut(|b| b.set_scroll(Scroll { line: new_line, vertical: 0.0, horizontal: 0.0 }))` → `editor.set_redraw(true)` を明示
- マウスホイールは cosmic_edit 既定 (`input.rs:238` の `Action::Scroll`) が効くので追加不要

**レイアウト調整 (重要):**

現状 `EDITOR_SIZE = Vec2(440, 320)` がエディタ Sprite の `custom_size` でもあり、root window 全体の幅も実質これに従属。Phase B は以下に分離:

- `EDITOR_PANEL_SIZE = Vec2(440.0, 320.0)` — 既存定数のリネーム、floating window の content_area サイズ
- `GUTTER_WIDTH = 36.0`, `SCROLLBAR_WIDTH = 8.0`
- `EDITOR_TEXT_SIZE = Vec2(EDITOR_PANEL_SIZE.x - GUTTER_WIDTH - SCROLLBAR_WIDTH, EDITOR_PANEL_SIZE.y)` — エディタ Sprite の custom_size はこちら

`spawn_strategy_editor_panel` で gutter (左 0..36) / editor (左 36..396) / scrollbar (左 396..404) を content_area の子として横並びに配置。content_area の中身は手動 transform で位置決め (既存 floating_window のパターン踏襲)。

### Phase C: Tab → spaces + auto-indent

**新規 `src/ui/strategy_editor_input.rs`:**

⚠️ **cosmic_edit の Enter は `ButtonInput<KeyCode>::just_pressed` 経由 (`input.rs:477`)**。`Events<KeyboardInput>.clear()` (menu_bar.rs Alt+F/E の手法) は **Enter には効かない** — ButtonInput resource と Events は別物。Enter を奪うには次のどれか:

1. (採用) **`ButtonInput<KeyCode>::reset(KeyCode::Enter)`** を我々のシステムで呼び、`before(bevy_cosmic_edit::input::InputSet)` を付ける
   - `Res<ButtonInput<KeyCode>>` ではなく `ResMut<ButtonInput<KeyCode>>` で取得 (mutable 必須)
   - `reset` は `just_pressed` を false に戻す (`bevy::input::ButtonInput` API)
2. (代替) cosmic_edit fork の `input.rs:477` を patch して、`StrategyEditorBypassEnter` のような Component を見つけたら Enter を無視するようにする — fork なので可能だが保守負担増
3. (代替) cosmic_edit の InputSet を `.run_if(not(editor_has_strategy_marker))` で完全に無効化し、必要な action 全部を自前で実装 — 過剰

→ **方式 1 で進める。**

- `tab_input_system`:
  - `Res<ButtonInput<KeyCode>>` で `KeyCode::Tab` just_pressed をチェック、`FocusedWidget == strategy_editor_entity` で発火
  - `editor.action(Action::Insert(' '))` を 4 回呼ぶ (cosmic_text の Editor API は char 単位、`insert_string` は `Editor::borrow_with(font_system)` 経由でしか呼べないため char ループの方が簡潔)
  - 起こりうる二重発火を防ぐため `before(bevy_cosmic_edit::input::InputSet)` を必ず付ける (cosmic_edit が Tab を Character 扱いしないとはいえ将来変更に備える)
- `enter_autoindent_system`:
  - `ResMut<ButtonInput<KeyCode>>` + `FocusedWidget` 一致 で `KeyCode::Enter` just_pressed をチェック
  - 前行の `&fragment.source` から `\n` 直前の行を取り出し、`len() - trim_start().len()` でインデント幅を抽出
  - `editor.action(Action::Insert('\n'))` → `editor.action(Action::Insert(' '))` を indent 幅ぶん繰り返す
  - **最後に `keys.reset(KeyCode::Enter)`** を呼んで cosmic_edit の Enter 処理を抑止
  - `before(bevy_cosmic_edit::input::InputSet)` 必須
- `bracket_autoclose_system`:
  - 入力文字が `(`, `[`, `{`, `"`, `'` のとき、**かつ次の文字が同じ closer (`)`, `]`, `}`, `"`, `'`) でないとき** のみ closer を後置 (`Action::Insert(closer)` → `Action::Motion(Motion::Left)`)
  - 選択範囲がある場合は「選択を囲む」(将来拡張、Phase C v1 では選択ありなら autoclose しないでスキップ)
  - コメント/文字列の中での autoclose 抑止は v2 (tree-sitter Tree から「いまカーソルがどの node の中か」を取れば判別できる、まずは無し)
  - **タイミング**: cosmic_edit 自身が opener (`(` 等) を `EventReader<KeyboardInput>` で読み挿入するので、我々は `.after(bevy_cosmic_edit::input::InputSet)` で動き、**`Events::clear()` は呼ばない** (cosmic_edit の opener 挿入を奪わない)。我々のシステムは `EventReader<KeyboardInput>` を**読むだけ** (clear せず) で文字種を判定し、cosmic_edit が opener を挿入した直後の cursor 位置に closer を後置する

### Phase D: Bracket match — Phase A に含めて完了

(設計上 highlight モジュールに同居させたので Phase D は省略)

### Phase E: Find / Replace

**新規 `src/ui/strategy_editor_find.rs`:**

- `FindReplaceState` Resource:
  ```rust
  pub struct FindReplaceState {
      pub query: String,
      pub replacement: String,
      pub case_sensitive: bool,
      pub matches: Vec<MatchSpan>,           // (entity 単位ではなく target_editor に紐づく)
      pub current: usize,
      pub is_open: bool,
      pub target_editor: Option<Entity>,     // 開いた瞬間に focused だった editor
  }
  pub struct MatchSpan {
      pub line: usize,       // source 行
      pub byte_range: std::ops::Range<usize>,
  }
  ```

- **Find エディタの専用マーカー (revised-v2 修正 — 重大8):**

  ```rust
  #[derive(Component)]
  pub struct FindQueryEditor;

  #[derive(Component)]
  pub struct FindReplacementEditor;
  ```

  Find パネルの 2 つの `CosmicEditBuffer` には **`StrategyEditorContent` を絶対に付けない**。代わりに上記の専用マーカーを付ける。

  なぜ重要か: 既存 `sync_editor_to_strategy_buffer_system` ([line 283–328](src/ui/strategy_editor.rs)) は `editor_q: Query<&StrategyEditorId, With<StrategyEditorContent>>` で絞っているので、`StrategyEditorContent` を付けないだけで自動的に Find editor の `CosmicTextChanged` は無視される。逆に、Navigator が雛形コピペで誤って `StrategyEditorContent` を付けると、Find への入力 1 文字ごとに対象 Strategy Editor の `fragment.source` がそれで上書きされる事故になる。明記必須。

  highlight 系 (`apply_tree_sitter_highlight` / `apply_find_match_highlight` / `apply_bracket_match_highlight`) も同じ理由で `With<StrategyEditorContent>` フィルタを使うので、Find editor は highlight 対象外になる (= プレーンテキストで表示される、それで OK)。

- `find_replace_ui_system` — **bevy_egui は現状 Cargo.toml に無いので、既存 `spawn_floating_window` ヘルパで世界空間の小型パネルを `is_open == true` の時だけ commands.spawn する**。パネル内に `CosmicEditBuffer` × 2 (query / replacement、各 `MaxLines(1)`) と、行/件数表示用 `Text2d`、「Prev」「Next」「Replace」「Replace All」用の Sprite + Pointer<Click> observer 4 個を子配置。bevy_egui を導入する案も検討余地はあるが、Phase 7.2 の最短ルートとしては既存パターンの再利用を優先する
- `find_match_recompute_system` — `FindReplaceState.query` 変更 or `Changed<StrategyFragment>` で全マッチ再計算 (まずは plain substring match、regex は v2)
- `apply_find_match_highlight_system` — マッチ列に `Attrs::color(text).background(FIND_MATCH_BG)` を AttrsList に重ね塗り (syntax の後、bracket の前)。最後に `set_redraw(true)` を明示。
- `find_scroll_to_match_system` — `current` 変更で `editor.with_buffer_mut(|b| b.set_scroll(Scroll { line: match.line.saturating_sub(viewport_lines / 2), vertical: 0.0, horizontal: 0.0 }))` で対象行を画面中央へ、`editor.set_redraw(true)`
- Cmd/Ctrl+F で開く: `ResMut<ButtonInput<KeyCode>>` で modifier + KeyF を見て `is_open = true` + `target_editor = focused_widget.0` をセット、**`FocusedWidget` を `FindQueryEditor` のエンティティに切り替える**
- Esc で閉じる + `FocusedWidget` を `target_editor` に戻す

**target_editor の lifecycle (重要):**

- find が開いている状態で対象 editor の panel が閉じられる/despawn される可能性 → `find_match_recompute_system` 冒頭で `editor_q.get(target_editor).is_err()` なら `FindReplaceState::default()` でリセット
- multi-spawn 時は「最後に focus していた editor」を対象とする (グローバル単一の FindReplaceState で十分、Zed の per-pane search は将来)

## 触るファイル一覧

**新規 (5 ファイル):**
- `src/ui/strategy_editor_highlight.rs`
- `src/ui/strategy_editor_gutter.rs`
- `src/ui/strategy_editor_scrollbar.rs`
- `src/ui/strategy_editor_input.rs`
- `src/ui/strategy_editor_find.rs`

**新規 (アセット):**
- `assets/queries/python_highlights.scm` — Zed の grammar query をコピー、**`;` で始まる出典コメント**付与 (revised-v2 修正)

**修正:**
- `Cargo.toml` — `tree-sitter`, `tree-sitter-python` 追加 (API バージョン確認後)
- `src/ui/mod.rs` — 5 モジュール宣言
- `src/main.rs` (または既存の plugin 集約点) — 5 モジュールのシステム登録 + 実行順序を **`chain()` で連結** (`sync_strategy_buffer_to_editor` → `sync_editor_to_strategy_buffer` → `apply_tree_sitter_highlight` → `apply_find_match_highlight` → `apply_bracket_match_highlight`)。**`SyntaxHighlighter` は `app.insert_non_send_resource(...)` で startup 初期化** (revised-v2 修正 — 重大1)。`add_systems` タプル 20 上限に注意 (現状の登録数を確認、超えたら chain で分割)
- `src/ui/strategy_editor.rs`:
  - `spawn_strategy_editor_panel` で gutter/scrollbar も spawn、Sprite サイズ計算を `EDITOR_PANEL_SIZE` / `EDITOR_TEXT_SIZE` に分離
  - `with_text` → `with_rich_text` (初期空 spans)
  - `set_wrap(Wrap::None)` を 1 回呼ぶ
  - `EDITOR_LINE_HEIGHT` を `EDITOR_METRICS` 定数 (`Metrics::new(14.0, 18.0)`) に格上げして全モジュールで共有 (`const` 不可なら `pub fn editor_metrics() -> Metrics`)
- `src/ui/components.rs` — syntax 色トークン (`SYNTAX_KEYWORD`/`STRING`/`COMMENT`/`FUNCTION`/`TYPE`/`NUMBER`/`OPERATOR`) + bracket / find 用 (`BRACKET_MATCH_FG`, `FIND_MATCH_BG`, `FIND_CURRENT_MATCH_BG`) を追加

**unchanged だが確認のみ:**
- `crates/bevy_cosmic_edit/src/input.rs` — `KeyCode::Tab` が `Key::Character` でないため `match` の `_ => ()` で吸われる動作の再確認 (line 497–508)、`InputSet` の存在 (line 31)
- `crates/bevy_cosmic_edit/src/buffer.rs` — `set_redraw` の public 性 (line 142, 160, 215, 246) 再確認
- `src/ui/editor_history.rs` — Undo/Redo 後の `PendingStrategySnapshotRestore` 経路で `fragment.source` が書き換わり、`Changed<StrategyFragment>` 経由で再ハイライトが走ることを確認

## 再利用する既存ピース

- `StrategyFragment` (`components.rs:312`) — source of truth、`.source` を `Changed` フィルタで購読する
- `StrategyBuffer` (`components.rs:105`) — `original_path` / `cache_path` のみ持つ Resource、Phase 7.2 では触らない
- `editor_history.rs` の `AppHistory` / `Record<AppEdit>` — Undo/Redo はそのまま
- `floating_window.rs::spawn_floating_window` — エディタパネルの枠はそのまま、content_area に gutter/scrollbar/editor を子配置
- `layout_persistence.rs` — Find パネルの開閉状態は永続化しない (セッションスコープ)、layout JSON の version 据え置き
- `bevy_cosmic_edit::CosmicEditBuffer::with_rich_text` (`crates/bevy_cosmic_edit/src/buffer.rs:120`) — Phase A の初期化のみ。以降は **`buffer.lines[i].set_attrs_list(...)`** で attrs だけ差し替え + **`buffer.set_redraw(true)` を明示**
- `bevy_egui` — **本プランでは使わない** (Cargo.toml にも未登録)。Find パネル含め全 UI を Sprite + Text2d + bevy_cosmic_edit の世界空間ウィンドウで揃える
- `Res<ButtonInput<KeyCode>>` + `FocusedWidget` 判定 — Tab/Enter/Ctrl+F の検出
- `EventReader<KeyboardInput>` (read-only) — bracket autoclose の文字判定。`Events::clear()` は **呼ばない** (cosmic_edit が opener を入れるのを邪魔しない、Caveat 5 参照)。`menu_bar.rs` Alt+F/E は cosmic_edit を完全に黙らせる用途で `clear()` を使っているが本タスクの用途とは違うので混同しない

## Caveat 一覧 (本タスクで踏みうるもの)

1. **`set_rich_text` はカーソルを (0,0) にリセット** — Phase A は初期化のみで使う。以降は `BufferLine::set_attrs_list` で attrs だけ更新する (cosmic_text 0.12 で API 確認済: `BufferLine::set_attrs_list(AttrsList) -> bool`)
2. **focused / unfocused で描画されるバッファが違う** — render.rs:88 のコメント通り、focused なら editor 内部 buffer、unfocused なら CosmicEditBuffer が描画される。`for_each_buffer` ヘルパは editor 側があれば editor のみ、無ければ CosmicEditBuffer のみを更新する (両更新は不要、focus 切替時に editor 側へ書き戻される設計)
3. **cosmic_edit Enter は `ButtonInput<KeyCode>::just_pressed` で読まれている** — `Events<KeyboardInput>.clear()` では止まらない。`ResMut<ButtonInput<KeyCode>>::reset(KeyCode::Enter)` を `.before(bevy_cosmic_edit::input::InputSet)` で呼ぶ
4. **Tab は cosmic_edit が黙って吸う** — `Key::Tab` は `Key::Character` ではないので `match _ => ()` で無視される。Phase C で `Action::Insert(' ') × 4` を発火させても二重発火しないが、将来防衛として `.before(InputSet)` は付ける
5. **bracket autoclose の順序は逆** — opener (`(`) は cosmic_edit に挿入させ、closer (`)`) を我々が後置する。`.after(bevy_cosmic_edit::input::InputSet)` で動かし、`Events<KeyboardInput>.clear()` は **絶対に呼ばない** (呼ぶと opener も入らなくなる)
6. **`Parser` は `!Sync`** — Bevy 0.15 の `Resource` は `Send + Sync` 必須なので **コンパイルが通らない**。**`world.insert_non_send_resource(SyntaxHighlighter { parser, query, ... })`** で登録し、system では **`NonSendMut<SyntaxHighlighter>`** で取り出す。代替の `Mutex<Parser>` は Bevy が Resource を独占する以上無駄ロック。previous Tree は `Local<HashMap<Entity, Tree>>` でキャッシュ (Tree は Send+Sync)。**system 冒頭で `prev_trees.retain(|e, _| editor_q.contains(*e));` を呼んで despawn entity を掃除** しないとメモリリーク (revised-v2 修正)
7. **`set_wrap` は `&mut FontSystem` 必須** — `editor_buffer.0.set_wrap(&mut font_system.0, Wrap::None)` の形で呼ぶ。`CosmicEditBuffer` 直叩きでも `&mut FontSystem` が要る。**Phase B の widget サイズ変更後に `Wrap::None` が維持されているか目視確認**、上書きされていれば毎フレーム再設定する system を追加 (revised-v2 補強)
8. **AttrsList 重ね順** — syntax → find → bracket の順で `.chain()` 連結、各段は前段の AttrsList をベースに `add_span` で重ねる。**システム全体の chain は `sync_strategy_buffer_to_editor → sync_editor_to_strategy_buffer → apply_tree_sitter_highlight → apply_find_match_highlight → apply_bracket_match_highlight`** (revised-v2 明示)
9. **`Scroll::line` は layout 行、`Scroll::vertical` は line 内 pixel** — wrap を None に固定すれば source 行 == layout 行で gutter と一致
10. **Undo/Redo 後の再ハイライト** — `fragment.source` を書き換える経路 (PendingStrategySnapshotRestore) を通れば自動で `Changed<StrategyFragment>` が立つ。dirty フィールドには触らない
11. **`fragment.dirty` は autosave 専用** — highlight 用には Bevy 標準の `Changed<StrategyFragment>` を使う (二重利用は競合の元)
12. **Find target_editor の lifecycle** — 開いた瞬間の `FocusedWidget.0` を `target_editor` に保存、Esc 時にそこへ戻す。target editor が despawn 済みなら state をリセット (`q.get(e).is_err()` チェック)
13. **`EditorScrollThumb` は `target_editor: Entity` を carry する** — multi-spawn で thumb が複数並ぶため、observer から「どのエディタを操作するか」を引けるようにする
14. **EDITOR_PANEL_SIZE と EDITOR_TEXT_SIZE を分離** — 既存 `EDITOR_SIZE` は panel サイズ意味で残し、エディタ Sprite には `EDITOR_TEXT_SIZE = panel - gutter - scrollbar` を渡す。混同するとパネルごと縮む
15. **`add_systems` タプル 20 上限** — Phase A〜E で 10 個以上のシステムを追加するので、既存登録数次第で chain 分割
16. **highlights.scm の出所** — `tree-sitter-python` Rust crate に同梱されていない可能性が高い。Zed の grammar query を `assets/queries/python_highlights.scm` にコピーする。**出典コメントは `;` で始める** (`//` だと `Query::new` パースエラー、revised-v2 修正)
17. **tree-sitter ABI** — `tree-sitter 0.24` + `tree-sitter-python 0.23` の組合せは ABI 14 で動くはずだが、`Parser::set_language` の戻り値型と引数型 (`&Language` か `Language` か) は API バージョンで変わる。作業前に crate README を再確認
18. **bevy_egui は Cargo.toml に無い** — Find パネルは egui を使わず、既存 floating window パターン (Sprite + CosmicEditBuffer × 2) で組む
19. **`StrategyFragment` と `CosmicEditBuffer` は別 entity** (revised-v2 追加 — 重大2) — root entity に `WindowRoot + StrategyFragment + StrategyEditorId`、child editor entity に `StrategyEditorContent + CosmicEditBuffer + Option<CosmicEditor> + StrategyEditorId`。1 つの Query で両方は取れない。**`fragments_q: Query<.., (With<WindowRoot>, Changed<StrategyFragment>)>` + `editor_q: Query<.., With<StrategyEditorContent>>` の 2 段ジョイン**を `StrategyEditorId.region_key` で行う (既存 sync 群と同じパターン)
20. **`BufferLine::set_attrs_list` 単独では再描画されない** (revised-v2 追加 — 重大3) — `set_attrs_list` 内部の `reset_shaping` は字形再計算フラグであって、render が見る Buffer 全体の `redraw` flag は別。attrs 更新後に **`buffer.set_redraw(true)`** (CosmicEditBuffer なら `b.set_redraw(true)`、editor 内部 buffer なら `editor.with_buffer_mut(|b| b.set_redraw(true))` + `editor.set_redraw(true)`) を明示
21. **Find editor は `StrategyEditorContent` を絶対に付けない** (revised-v2 追加 — 重大8) — 専用マーカー `FindQueryEditor` / `FindReplacementEditor` を使う。誤って `StrategyEditorContent` を付けると Find 入力の `CosmicTextChanged` が `sync_editor_to_strategy_buffer_system` に拾われ、Strategy Editor の `fragment.source` が Find 文字列で上書きされる事故になる
22. **bracket match キャッシュは別行をまたぐ** (revised-v2 追加 — 重大7) — opener と closer は通常別行にある。`Local<Option<(Entity, usize, Range<usize>)>>` (1 行 1 範囲) では不足、**`Local<HashMap<Entity, Vec<(usize, Range<usize>)>>>`** (通常 2 要素) を採用してクリーンアップ時に両方の行に対して syntax 色を再適用 + `set_redraw(true)` する
23. **incremental parse には `Tree::edit(&InputEdit)` が必須** (revised-v2 追加 — 重大6) — `parser.parse(source, Some(&prev_tree))` だけでは前回ツリーから何も再利用されず、実質フル再パースになる。Phase A v1 は **フル再パース**で実装 (Python 1KB 数 ms で十分実用)。incremental を本気で効かせるには `CosmicTextChanged` 経路で diff 情報 (`InputEdit`) を作って配信する設計拡張が必要 — Phase A v2 の対象、v1 では触らない

## Verification (各フェーズ完了時)

### コンパイル & 単体テスト
```bash
cargo check
cargo test --lib
```

### E2E 手動検証 (`e2e-testing` スキル併用)
1. `cargo run --bin backcast` で起動
2. Strategy Editor (Sidebar から spawn) → `python/tests/data/test_strategy_daily.py` を Ctrl+O でロード
3. **Phase A**: 
   - `def`, `class`, `import`, 文字列, コメントが色分け
   - カーソル前の `(` に対応する `)` がハイライトされる
   - **opener と closer が別行にあっても両方ハイライトされ、カーソルが動いたら両方クリアされる** (revised-v2 追加)
   - **長文をタイプしてもカーソルが先頭に飛ばない**
   - **Undo/Redo (Ctrl+Z/Y) 直後にも色が抜けない、白いフラッシュが 1 フレームも出ない** (revised-v2 追加 — 順序の検証)
4. **Phase B**: 
   - 左に行番号、右に scrollbar thumb が表示されスクロールに追従
   - thumb をドラッグして移動できる
   - 行番号がエディタの行高にズレなく揃う
   - **長い行 (200 文字以上の 1 行) を入力したときに折り返されず横にはみ出すこと** (revised-v2 追加 — `Wrap::None` が widget 経由で上書きされていないことの確認)
5. **Phase C**: Tab で 4 spaces 入る、Enter 後に前行のインデントが継承される、`(` で `)` が自動補完されカーソルが間に残る、`)` の前で `(` を打っても `))` にならない
6. **Phase E**: 
   - Ctrl+F で Find パネル開く、`def` を検索して全マッチ強調 + 最初のマッチへスクロール
   - Enter で次へ、Esc で閉じてエディタにフォーカス戻る
   - find 中に対象エディタを × で閉じてもクラッシュしない
   - **Find パネルの query 欄に文字を打っても、対象 Strategy Editor の本文が書き換わらない** (revised-v2 追加 — マーカー分離の確認)
7. Undo (Ctrl+Z) 後に色付け・行番号・スクロールバーすべて正しく再描画される
8. Multi-spawn (region_002 等) でも各エディタに独立して上記が動く

### 既存機能の非退行
- Auto-save (1 秒 debounce) が引き続き動く (`fragment.dirty` 経路に手を入れていないこと)
- ドラッグでウィンドウ移動 + Undo
- Ctrl+S/O での Save/Load (layout JSON 経由)
- Sidecar JSON (`<strategy>.json`) との連携

## 実装方針メモ

- **pair-relay 移行候補**: Phase A だけで attrs API の地ならしで 250〜400 行、全フェーズ完遂は 1 セッションでは厳しい。Phase A 着手前に `pair-relay` スキルへ移行、本プランを Navigator に引き継ぐのが安全。Navigator は事前に `bevy-engine` スキルで Bevy 0.15 罠 (NonSendResource の登録方法、observer の import path、Anchor 左寄せ) を、`zed` スキルで attrs 重ね塗りの先行事例 (`syntax_theme.rs` / `syntax_map.rs`) を必ず読む
- **Bevy 0.15 罠**: `add_systems` タプル 20 上限、`world.insert_non_send_resource` vs `app.insert_resource` の差、observer の import path (`bevy::ecs::observer::Trigger`)、Anchor 左寄せ (`bevy::sprite::Anchor::CenterLeft`) は `bevy-engine` スキル発動で都度確認
- **tree-sitter API 確認手順 (revised-v2 補強)**: 着手 1 コミット目で `examples/tree_sitter_smoke.rs` を作り、以下を `cargo run --example` で先に確認する。crate API が想定と違ったら設計に戻る。
  1. `Parser::set_language` の引数型 (`&Language` か `Language` か)
  2. `tree_sitter_python::LANGUAGE` (定数) vs `tree_sitter_python::language()` (関数) のどちらが正か
  3. `QueryCursor::matches` の戻り値型 (lifetime / iterator か)
  4. `Tree::edit(&InputEdit)` のシグネチャ (Phase A v2 で使う)
  5. **`Parser` が実際に `!Sync` であることを `static_assertions::assert_not_impl_all!(tree_sitter::Parser: Sync);` で確認**
- **Zed grammar のライセンス確認**: `.claude/skills/zed/src/crates/languages/src/python/highlights.scm` を `assets/queries/python_highlights.scm` にコピー時、Zed リポジトリのライセンス (Apache-2.0 / GPL-3.0 dual) を確認。クエリ単体の派生は Apache-2.0 系で扱えるが、念のため冒頭に `; Source: zed-industries/zed crates/languages/src/python/highlights.scm` と `; Upstream license: Apache-2.0` を **`;` コメント形式で**追加

## v2 改訂サマリ (レビュー指摘 8 点の取り込み箇所)

| 指摘 | 取り込み箇所 |
|---|---|
| 重大1: Parser !Sync | アーキ概要 / Phase A `SyntaxHighlighter` 節 / Caveat 6 / 触るファイル一覧 (main.rs) / 実装方針メモ (smoke test 5) |
| 重大2: root/child Query 分離 | アーキ概要「既存 ECS 構造」節 / Phase A `apply_tree_sitter_highlight_system` コード例 / Caveat 19 |
| 重大3: set_redraw 明示 | アーキ概要「attrs 専用更新」節 step 3 / Phase A `apply_tree_sitter_highlight_system` 2e / Phase B `update_gutter_text_system` / Phase E `apply_find_match_highlight_system` / Caveat 20 |
| 重大4: sync 系との順序 | アーキ概要「システム実行順序」節 / Phase A システム登録節 (`chain()`) / Caveat 8 / Verification Phase A step 3 (Undo 白フラッシュ確認) |
| 重大5: wrap の widget 上書き | Phase B `wrap モード` 節 ⚠ 注記 / Caveat 7 補強 / Verification Phase B step 4 (長い行はみ出し確認) |
| 重大6: 偽 incremental | Phase A `apply_tree_sitter_highlight_system` 2b コメント / `Phase A v1 と v2 の境界` 節 / Caveat 23 |
| 重大7: bracket cache 別行 | Phase A `Bracket match` 節「前回ハイライト範囲の保持形式」/ Caveat 22 / Verification Phase A step 3 (opener/closer 別行ハイライト) |
| 重大8: Find editor 区別 | アーキ概要 責務分割表の find 行 / Phase E `Find エディタの専用マーカー` 節 / Caveat 21 / Verification Phase E step 6 (本文書き換わらない確認) |
| 軽微: scm コメント `;` | Phase A `highlights.scm の取得` 節 ⚠ 注記 / 触るファイル一覧 (assets) / Caveat 16 / 実装方針メモ (Zed grammar) |
