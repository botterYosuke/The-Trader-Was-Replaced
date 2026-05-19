# Strategy Editor を Monaco-grade に格上げする (Zed 参照)

## Context

`src/ui/strategy_editor.rs` (1254 行) は現状、bevy_cosmic_edit ベースの**単色テキストエディタ**にすぎない。基本機能 (undo/redo / 自動保存 / フラグメント管理 / multi-spawn / レイアウト永続化) は完成しているが、**ソースコードエディタとしての視認性・編集体験**が欠けている — syntax highlight も行番号もスクロールバーも Find も無い。Tab は cosmic_edit の input ハンドラが `Key::Character` でない logical key を無視するため何も起きない (= 入りも focus 遷移もしない、無音で吸われる)。

ユーザは「Zed を参考に高級ソースコードエディタに変えたい」。スコープは **Monaco-grade** で合意済 (LSP・診断・command palette・マルチカーソルは含めず、syntax / gutter / scrollbar / find&replace / auto-indent / Tab→spaces / bracket match まで)。Syntax highlight は **tree-sitter-python** ベースで合意。

`.claude/skills/zed/SKILL.md` に「Monaco-grade Strategy Editor は実装済」と書かれているが、これは 0.15 移行時に**実態は剥がれている** (今回の作業で実装し直す)。Phase 7.2 完了時にスキル本文も同期する。

## アーキテクチャ概要

`strategy_editor.rs` を肥大化させず、**5 つの新規モジュール**に責務を分割。既存の `StrategyBuffer` (Resource、`source` は持たない — `original_path` / `cache_path` のみ) と `StrategyFragment` (Component、`source: String` + `dirty: bool`) を**唯一の source of truth** として、各モジュールは「source が変わったら派生表示を再計算」する設計。

### `StrategyFragment.dirty` の二重利用回避 (重要)

既存コードでは `fragment.dirty = true` は「autosave が拾うべき変更がある」を意味する (`debounced_strategy_autosave_system` が見て書き出し後 false に戻す)。Highlight 再計算のトリガに同じフラグを流用すると「autosave が先に dirty を落とすと highlight が走らない」競合になる。

→ **highlight 系は Bevy 標準の `Changed<StrategyFragment>` (フィルタ) を使う**。`dirty: bool` フィールドは autosave 専用のまま据え置く。

### 責務分割と Zed 参照対応

| 新規モジュール (`src/ui/`) | 責務 | Zed 参考 (`.claude/skills/zed/src/`) |
|---|---|---|
| `strategy_editor_highlight.rs` | tree-sitter-python で AST 取得 → highlight query で span 列を生成 → `BufferLine::set_attrs_list` で各行の attrs **だけ**を差し替え (テキストは触らない = カーソル不変) | `crates/syntax_theme/src/syntax_theme.rs` (capture→color マップ), `crates/language/src/syntax_map.rs` (snapshot 戦略) |
| `strategy_editor_gutter.rs` | エディタ左に独立 cosmic_text `Buffer` をもう 1 つ持つ Sprite を配置、行番号文字列を同じ Metrics で描画 | `crates/editor/src/element.rs` (paint_gutter / layout_gutter) |
| `strategy_editor_scrollbar.rs` | エディタ右に `Sprite` で thumb を描画。Pointer<Drag> で scroll を変更 | `crates/editor/src/scroll.rs` (ScrollAnchor + offset), `scroll/autoscroll.rs` (strategy enum) |
| `strategy_editor_input.rs` | Tab → 4 spaces, Enter → 前行インデント継承, 括弧キーで自動閉じ | `crates/editor/src/editor.rs::tab`, `editor::newline`, `crates/language/src/language.rs::indent_size_for_line` |
| `strategy_editor_find.rs` | 世界空間の小型パネル (`Sprite` + `CosmicEditBuffer` × 2 = query / replacement) を `spawn_floating_window` ヘルパで配置、マッチ行の attrs を上書き、Enter/F3 で次へ | `crates/search/src/buffer_search.rs` (BufferSearchBar の query/replacement editor 分離) |

`bracket_match` (現在カーソル位置の対応括弧ハイライト) は `strategy_editor_highlight.rs` 内に小さな関数として同居 (Zed の `highlight_matching_bracket.rs` を参考、innermost bracket スキャンを cursor 周辺だけ)。

### attrs 専用更新で「カーソルリセット問題」を回避 (Critical)

`CosmicEditBuffer::set_rich_text` は内部で `Buffer::set_rich_text` を呼び、buffer.lines を**全部作り直す**ため `editor.action(Action::Click ...)` を別途呼ばないとカーソルが (0,0) にリセットされる。これは既存 `strategy_editor.rs:228` のコメントが既に警告している既知の foot-gun。

→ **テキストは触らず attrs だけ差し替える**。具体的には:

1. cosmic_text `Buffer::lines: Vec<BufferLine>` を `with_buffer_mut(|b| b.lines.iter_mut())` で借り、
2. 各 `BufferLine::set_attrs_list(AttrsList)` を呼んで line ごとの spans を更新、
3. 同時に `BufferLine` の `set_redraw(true)` 相当の dirty 化を行う (cosmic_text 0.x では `set_attrs_list` 内部で reset_shaping される)。

これにより:
- buffer の line 構造は不変 → カーソル位置・選択範囲は保たれる
- 表示色のみ変わる → 1 フレームで再描画される

**`set_rich_text` を使うのは初回 spawn 時のみ** (`with_rich_text` で空 spans を渡し、初期色付けは `Changed<StrategyFragment>` の起動で 1 フレーム後に流す)。

### システム実行順序 (極めて重要)

cosmic_text の `AttrsList` は **後から上書きしたものが勝つ**。下の順で `after()` を付ける:

```
1. apply_tree_sitter_highlight   (Changed<StrategyFragment> 駆動 — 全行 attrs 差し替え)
2. apply_find_match_highlight    (FindReplaceState 駆動 — マッチ範囲の attrs 上書き)
3. apply_bracket_match_highlight (cursor 位置駆動 — 2 文字だけ attrs 上書き)
```

各システムは `BufferLine::set_attrs_list` で前段の AttrsList を**置き換える**のではなく、前段の結果をベースに `AttrsList::add_span` で重ね塗りする (`AttrsList` は `Cow<'_, AttrsList>` 風に持ち回せる)。逆順だと find マッチ色や bracket 色が syntax 色で消える (zed スキル Caveat 6)。

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

- (推奨) `.claude/skills/zed/src/crates/grammars/src/python/highlights.scm` をリポジトリ内の `assets/queries/python_highlights.scm` にコピーして `include_str!("../../assets/queries/python_highlights.scm")` で読む — ライセンス確認 (Zed grammars は MIT) + 出典コメント必須
- `tree-sitter-python` の `Cargo.toml::include` フィールドを確認して `queries/highlights.scm` が同梱されていれば `concat!(env!("CARGO_MANIFEST_DIR"), "/../tree-sitter-python/queries/highlights.scm")` のような workspace 内 path で参照 (ただし依存 crate の内部パスに依存するため脆い)

**新規 `src/ui/strategy_editor_highlight.rs`:**

- `SyntaxHighlighter` Resource — `tree_sitter::Parser` + コンパイル済 `Query`。`Parser::parse` は `&mut self` を取り **`!Sync` かつシリアル前提**。複数 editor が存在しても 1 つの Resource を順に使い回す (Bevy system は単一スレッドで Resource を独占するので問題なし、ただし `Local<HashMap<Entity, Tree>>` で前回の `Tree` を保持しておけば incremental parse が効く)
- `SyntaxTreeCache` Resource (オプション) — Local だと editor despawn 時にエントリが永久に残るので、**despawn 時に観測子で purge**: `app.add_observer(|trigger: Trigger<OnRemove, StrategyFragment>, mut cache: ResMut<SyntaxTreeCache>| { cache.0.remove(&trigger.target()); })` を登録 (Bevy 0.15 で `add_observer` API、`Trigger::target` で entity 取得)。Local<HashMap> でも `cache.retain(|e, _| world.get_entity(*e).is_ok())` のようなフレーム末プルーニングは可能だが、observer の方が綺麗
- `HighlightTheme` Resource — capture 名 (`"keyword"`, `"string"`, `"function"`, `"comment"`, `"number"`, `"type"`, ...) → `cosmic_text::Color` のマップ。**色は新規追加**: `src/ui/components.rs` に Dracula 風の `SYNTAX_KEYWORD` / `SYNTAX_STRING` / `SYNTAX_COMMENT` / `SYNTAX_FUNCTION` / `SYNTAX_TYPE` / `SYNTAX_NUMBER` / `SYNTAX_OPERATOR` を追加 (既存の panel/button 色とは独立の syntax 名前空間)
- `apply_tree_sitter_highlight_system` — `Query<(Entity, &StrategyFragment, &mut CosmicEditBuffer, Option<&mut CosmicEditor>), Changed<StrategyFragment>>` で発火:
  1. `Parser::parse(&fragment.source, previous_tree.as_ref())` で AST 取得 (incremental)
  2. `QueryCursor::matches` でキャプチャ列挙、`(byte_start, byte_end, capture_name)` を `Vec` に集約
  3. ソースを `\n` で split しながら、行ごとに該当 captures だけを抽出 → `AttrsList::add_span(local_start..local_end, attrs)` で line 用 AttrsList を構築
  4. `for_each_buffer(entity, |buffer| { buffer.lines[i].set_attrs_list(list.clone()); })` で両 buffer に反映
- Startup で `Parser::set_language` を 1 回実行 + Query をコンパイル (Query コンパイルは数 ms、初回のみ)
- previous Tree 保持: `Local<HashMap<Entity, tree_sitter::Tree>>` (システム内 cache、ECS resource にしない)

**Bracket match (highlight モジュール同居):**

- `apply_bracket_match_highlight_system` — **毎フレーム実行** ( `Local<HashMap<Entity, Cursor>>` で前回値を持ち、現在の `editor.cursor()` (cosmic_text Editor の `Deref` 経由) と比較して変化なしならスキップ)。`CosmicTextChanged` だけでは不十分な理由: cosmic_edit のイベントは `is_edit = true` の時にしか飛ばない (input.rs:518)、つまり矢印キー/クリックのみのカーソル移動では飛ばない。focused editor を `Query<(Entity, &CosmicEditor), With<StrategyEditorMarker>>` で取り、`FocusedWidget.0` と一致したものだけ処理
- カーソル位置周辺で `(){}[]` の対応をスキャン (innermost、AST 不要、最大 4096 文字くらいで打ち切り)
- マッチ 2 文字に `Attrs::color(BRACKET_MATCH_COLOR)` を **AttrsList::add_span で重ね塗り** (前段の syntax color の上)
- 前回ハイライトした 2 文字の attrs をクリーンに戻すため、`Local<Option<(Entity, usize, Range<usize>)>>` で「前回どの行のどの範囲に bracket attrs を載せたか」を保持し、cursor が動いた瞬間にその range の attrs を syntax 由来に戻してから新しい範囲に載せる

**修正: `src/ui/strategy_editor.rs`**
- 初期 `with_text` → `with_rich_text([("", default_attrs)], default_attrs)` に置換 (Phase A 時点では空 spans でも、`set_attrs_list` 経由更新が走る前提)
- `sync_strategy_buffer_to_editor_system:262` 周辺の `set_text` 経路は変更しない (set_text 後は Changed<StrategyFragment> が立つので別系統で再ハイライトされる)

### Phase B: 行番号ガター + スクロールバー

**新規 `src/ui/strategy_editor_gutter.rs`:**

- `LineNumberGutter` Component — エディタ左 36px (font_size 14 で 5 桁分 + padding) に `Sprite` (背景) + **もう 1 つの独立 `CosmicEditBuffer`** (read-only、`ReadOnly` component 付加) を子として配置
  - 別 cosmic_text Buffer を持つことで、Metrics をエディタと完全一致させ、行の高さズレを根本的に排除
  - 共通定数 `EDITOR_METRICS: Metrics = Metrics::new(14.0, 18.0)` を `strategy_editor.rs` に置き、ガター/エディタ/find 全部で共有
- `update_gutter_text_system` — `Changed<StrategyFragment>` で `(1..=line_count).map(|i| format!("{i:>4}")).join("\n")` を gutter buffer に `set_text`
- スクロール追従: エディタ側の `editor.with_buffer(|b| { (b.scroll().line, b.scroll().vertical) })` を読み、ガター buffer の `set_scroll` に同じ値を入れる (line + vertical 両方コピー必須)
- **wrap モード**: エディタを `cosmic_text::Wrap::None` に固定する。`Buffer::set_wrap(&mut self, font_system: &mut FontSystem, wrap: Wrap)` は `FontSystem` 必須なので、startup 時に `Res<CosmicFontSystem>` から借りて `editor_buffer.0.set_wrap(&mut font_system.0, Wrap::None)` を 1 回呼ぶ (gutter 用の Buffer にも同様)。これで「source 行 == layout 行」になり、ガター行番号と scrollbar の line 数が一致する。長い行は横スクロール (cosmic_edit が `XOffset` で対応)

**新規 `src/ui/strategy_editor_scrollbar.rs`:**

- `EditorScrollThumb { target_editor: Entity }` Component — エディタ右 8px に `Sprite`。`target_editor` フィールドで「どのエディタを操作する thumb か」を保持する (multi-spawn で複数 thumb が並ぶため必須)
- thumb サイズ: `thumb_h = (viewport_lines / total_lines).clamp(0.05, 1.0) * scrollbar_h`
- thumb 位置: `(scroll.line as f32 / (total_lines - viewport_lines).max(1) as f32) * (scrollbar_h - thumb_h)`
  - `Scroll::vertical` (line 内 pixel 微調整) は thumb 位置には反映しない (微小なので無視)
- `Pointer<Drag>` observer で thumb を縦ドラッグ → `trigger.target()` で thumb entity を取得 → そこから `EditorScrollThumb::target_editor` を引いて対象エディタを特定 → drag.delta.y を line に逆換算 → `editor.with_buffer_mut(|b| b.set_scroll(Scroll { line: new_line, vertical: 0.0, horizontal: 0.0 }))`
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
- `find_replace_ui_system` — **bevy_egui は現状 Cargo.toml に無いので、既存 `spawn_floating_window` ヘルパで世界空間の小型パネルを `is_open == true` の時だけ commands.spawn する**。パネル内に `CosmicEditBuffer` × 2 (query / replacement、各 `MaxLines(1)`) と、行/件数表示用 `Text2d`、「Prev」「Next」「Replace」「Replace All」用の Sprite + Pointer<Click> observer 4 個を子配置。bevy_egui を導入する案も検討余地はあるが、Phase 7.2 の最短ルートとしては既存パターンの再利用を優先する
- `find_match_recompute_system` — `FindReplaceState.query` 変更 or `Changed<StrategyFragment>` で全マッチ再計算 (まずは plain substring match、regex は v2)
- `apply_find_match_highlight_system` — マッチ列に `Attrs::color(text).background(FIND_MATCH_BG)` を AttrsList に重ね塗り (syntax の後、bracket の前)
- `find_scroll_to_match_system` — `current` 変更で `editor.with_buffer_mut(|b| b.set_scroll(Scroll { line: match.line.saturating_sub(viewport_lines / 2), vertical: 0.0, horizontal: 0.0 }))` で対象行を画面中央へ
- Cmd/Ctrl+F で開く: `ResMut<ButtonInput<KeyCode>>` で modifier + KeyF を見て `is_open = true` + `target_editor = focused_widget.0`
- Esc で閉じる + `FocusedWidget` を `target_editor` に戻す

**target_editor の lifecycle (重要):**

- find が開いている状態で対象 editor の panel が閉じられる/despawn される可能性 → `find_match_recompute_system` 冒頭で `q.get(target_editor).is_err()` なら `FindReplaceState::default()` でリセット
- multi-spawn 時は「最後に focus していた editor」を対象とする (グローバル単一の FindReplaceState で十分、Zed の per-pane search は将来)

## 触るファイル一覧

**新規 (5 ファイル):**
- `src/ui/strategy_editor_highlight.rs`
- `src/ui/strategy_editor_gutter.rs`
- `src/ui/strategy_editor_scrollbar.rs`
- `src/ui/strategy_editor_input.rs`
- `src/ui/strategy_editor_find.rs`

**新規 (アセット):**
- `assets/queries/python_highlights.scm` — Zed の MIT-licensed クエリをコピー、出典コメント付与

**修正:**
- `Cargo.toml` — `tree-sitter`, `tree-sitter-python` 追加 (API バージョン確認後)
- `src/ui/mod.rs` — 5 モジュール宣言
- `src/main.rs` (または既存の plugin 集約点) — 5 モジュールのシステム登録 + 実行順序 (`apply_tree_sitter_highlight` → `apply_find_match_highlight` → `apply_bracket_match_highlight`)。`add_systems` タプル 20 上限に注意 (現状の登録数を確認、超えたら chain で分割)
- `src/ui/strategy_editor.rs`:
  - `spawn_strategy_editor_panel` で gutter/scrollbar も spawn、Sprite サイズ計算を `EDITOR_PANEL_SIZE` / `EDITOR_TEXT_SIZE` に分離
  - `with_text` → `with_rich_text` (初期空 spans)
  - `set_wrap(Wrap::None)` を 1 回呼ぶ
  - `EDITOR_LINE_HEIGHT` を `EDITOR_METRICS` 定数 (`Metrics::new(14.0, 18.0)`) に格上げして全モジュールで共有 (`const` 不可なら `pub fn editor_metrics() -> Metrics`)
- `src/ui/components.rs` — syntax 色トークン (`SYNTAX_KEYWORD`/`STRING`/`COMMENT`/`FUNCTION`/`TYPE`/`NUMBER`/`OPERATOR`) + bracket / find 用 (`BRACKET_MATCH_FG`, `FIND_MATCH_BG`, `FIND_CURRENT_MATCH_BG`) を追加

**unchanged だが確認のみ:**
- `crates/bevy_cosmic_edit/src/input.rs` — `KeyCode::Tab` が `Key::Character` でないため `match` の `_ => ()` で吸われる動作の再確認 (line 497–508)
- `src/ui/editor_history.rs` — Undo/Redo 後の `PendingStrategySnapshotRestore` 経路で `fragment.source` が書き換わり、`Changed<StrategyFragment>` 経由で再ハイライトが走ることを確認

## 再利用する既存ピース

- `StrategyFragment` (`components.rs:312`) — source of truth、`.source` を `Changed` フィルタで購読する
- `StrategyBuffer` (`components.rs:105`) — `original_path` / `cache_path` のみ持つ Resource、Phase 7.2 では触らない
- `editor_history.rs` の `AppHistory` / `Record<AppEdit>` — Undo/Redo はそのまま
- `floating_window.rs::spawn_floating_window` — エディタパネルの枠はそのまま、content_area に gutter/scrollbar/editor を子配置
- `layout_persistence.rs` — Find パネルの開閉状態は永続化しない (セッションスコープ)、layout JSON の version 据え置き
- `bevy_cosmic_edit::CosmicEditBuffer::with_rich_text` (`crates/bevy_cosmic_edit/src/buffer.rs:120`) — Phase A の初期化のみ。以降は **`buffer.lines[i].set_attrs_list(...)`** で attrs だけ差し替え
- `bevy_egui` — **本プランでは使わない** (Cargo.toml にも未登録)。Find パネル含め全 UI を Sprite + Text2d + bevy_cosmic_edit の世界空間ウィンドウで揃える
- `Res<ButtonInput<KeyCode>>` + `FocusedWidget` 判定 — Tab/Enter/Ctrl+F の検出
- `EventReader<KeyboardInput>` (read-only) — bracket autoclose の文字判定。`Events::clear()` は **呼ばない** (cosmic_edit が opener を入れるのを邪魔しない、Caveat 5 参照)。`menu_bar.rs` Alt+F/E は cosmic_edit を完全に黙らせる用途で `clear()` を使っているが本タスクの用途とは違うので混同しない

## Caveat 一覧 (本タスクで踏みうるもの)

1. **`set_rich_text` はカーソルを (0,0) にリセット** — Phase A は初期化のみで使う。以降は `BufferLine::set_attrs_list` で attrs だけ更新する (cosmic_text 0.12 で API 確認済: `BufferLine::set_attrs_list(AttrsList) -> bool`)
2. **focused / unfocused で描画されるバッファが違う** — render.rs:88 のコメント通り、focused なら editor 内部 buffer、unfocused なら CosmicEditBuffer が描画される。`for_each_buffer` ヘルパは editor 側があれば editor のみ、無ければ CosmicEditBuffer のみを更新する (両更新は不要、focus 切替時に editor 側へ書き戻される設計)
3. **cosmic_edit Enter は `ButtonInput<KeyCode>::just_pressed` で読まれている** — `Events<KeyboardInput>.clear()` では止まらない。`ResMut<ButtonInput<KeyCode>>::reset(KeyCode::Enter)` を `.before(bevy_cosmic_edit::input::InputSet)` で呼ぶ
4. **Tab は cosmic_edit が黙って吸う** — `Key::Tab` は `Key::Character` ではないので `match _ => ()` で無視される。Phase C で `Action::Insert(' ') × 4` を発火させても二重発火しないが、将来防衛として `.before(InputSet)` は付ける
5. **bracket autoclose の順序は逆** — opener (`(`) は cosmic_edit に挿入させ、closer (`)`) を我々が後置する。`.after(bevy_cosmic_edit::input::InputSet)` で動かし、`Events<KeyboardInput>.clear()` は **絶対に呼ばない** (呼ぶと opener も入らなくなる)
6. **`Parser` は `!Sync`、Bevy Resource 経由のシリアル使用が前提** — 複数 editor でも 1 つを使い回す。previous Tree は `Local<HashMap<Entity, Tree>>` or `Resource<SyntaxTreeCache>` でキャッシュ。**`OnRemove, StrategyFragment` observer で entry を purge** しないと editor despawn 後にエントリが残り続けてメモリリーク
7. **`set_wrap` は `&mut FontSystem` 必須** — `editor_buffer.0.set_wrap(&mut font_system.0, Wrap::None)` の形で呼ぶ。`CosmicEditBuffer` 直叩きでも `&mut FontSystem` が要る
8. **AttrsList 重ね順** — syntax → find → bracket の順で `.after()` 連結、各段は前段の AttrsList をベースに `add_span` で重ねる
9. **`Scroll::line` は layout 行、`Scroll::vertical` は line 内 pixel** — wrap を None に固定すれば source 行 == layout 行で gutter と一致
10. **Undo/Redo 後の再ハイライト** — `fragment.source` を書き換える経路 (PendingStrategySnapshotRestore) を通れば自動で `Changed<StrategyFragment>` が立つ。dirty フィールドには触らない
11. **`fragment.dirty` は autosave 専用** — highlight 用には Bevy 標準の `Changed<StrategyFragment>` を使う (二重利用は競合の元)
12. **Find target_editor の lifecycle** — 開いた瞬間の `FocusedWidget.0` を `target_editor` に保存、Esc 時にそこへ戻す。target editor が despawn 済みなら state をリセット (`q.get(e).is_err()` チェック)
13. **`EditorScrollThumb` は `target_editor: Entity` を carry する** — multi-spawn で thumb が複数並ぶため、observer から「どのエディタを操作するか」を引けるようにする
14. **EDITOR_PANEL_SIZE と EDITOR_TEXT_SIZE を分離** — 既存 `EDITOR_SIZE` は panel サイズ意味で残し、エディタ Sprite には `EDITOR_TEXT_SIZE = panel - gutter - scrollbar` を渡す。混同するとパネルごと縮む
15. **`add_systems` タプル 20 上限** — Phase A〜E で 10 個以上のシステムを追加するので、既存登録数次第で chain 分割
16. **highlights.scm の出所** — `tree-sitter-python` Rust crate に同梱されていない可能性が高い。Zed の MIT licensed クエリを `assets/queries/python_highlights.scm` にコピーする (出典コメント必須)
17. **tree-sitter ABI** — `tree-sitter 0.24` + `tree-sitter-python 0.23` の組合せは ABI 14 で動くはずだが、`Parser::set_language` の戻り値型と引数型 (`&Language` か `Language` か) は API バージョンで変わる。作業前に crate README を再確認
18. **bevy_egui は Cargo.toml に無い** — Find パネルは egui を使わず、既存 floating window パターン (Sprite + CosmicEditBuffer × 2) で組む

## Verification (各フェーズ完了時)

### コンパイル & 単体テスト
```bash
cargo check
cargo test --lib
```

### E2E 手動検証 (`e2e-testing` スキル併用)
1. `cargo run --bin backcast` で起動
2. Strategy Editor (Sidebar から spawn) → `python/tests/data/test_strategy_daily.py` を Ctrl+O でロード
3. **Phase A**: `def`, `class`, `import`, 文字列, コメントが色分け、カーソル前の `(` に対応する `)` がハイライトされる、**長文をタイプしてもカーソルが先頭に飛ばない**
4. **Phase B**: 左に行番号、右に scrollbar thumb が表示されスクロールに追従、thumb をドラッグして移動できる、行番号がエディタの行高にズレなく揃う
5. **Phase C**: Tab で 4 spaces 入る、Enter 後に前行のインデントが継承される、`(` で `)` が自動補完されカーソルが間に残る、`)` の前で `(` を打っても `))` にならない
6. **Phase E**: Ctrl+F で Find パネル開く、`def` を検索して全マッチ強調 + 最初のマッチへスクロール、Enter で次へ、Esc で閉じてエディタにフォーカス戻る、find 中に対象エディタを × で閉じてもクラッシュしない
7. Undo (Ctrl+Z) 後に色付け・行番号・スクロールバーすべて正しく再描画される
8. Multi-spawn (region_002 等) でも各エディタに独立して上記が動く

### 既存機能の非退行
- Auto-save (1 秒 debounce) が引き続き動く (`fragment.dirty` 経路に手を入れていないこと)
- ドラッグでウィンドウ移動 + Undo
- Ctrl+S/O での Save/Load (layout JSON 経由)
- Sidecar JSON (`<strategy>.json`) との連携

## 実装方針メモ

- **pair-relay 移行候補**: Phase A だけで attrs API の地ならしで 250〜400 行、全フェーズ完遂は 1 セッションでは厳しい。Phase A 着手前に `pair-relay` スキルへ移行、本プランを Navigator に引き継ぐのが安全。Navigator は事前に `bevy-engine` スキルで Bevy 0.15 罠を、`flowsurface` スキルで attrs 重ね塗りの先行事例を必ず読む
- **Bevy 0.15 罠**: `add_systems` タプル 20 上限、observer の import path (`bevy::ecs::observer::Trigger`)、Anchor 左寄せ (`bevy::sprite::Anchor::CenterLeft`) は `bevy-engine` スキル発動で都度確認
- **tree-sitter API 確認手順**: 着手 1 コミット目で `examples/tree_sitter_smoke.rs` を作り、`Parser::set_language` と `QueryCursor::matches` が `cargo run --example` で動くかだけ先に確認する。crate API が想定と違ったら設計に戻る
- **Zed grammar のライセンス確認**: `.claude/skills/zed/src/crates/grammars/src/python/highlights.scm` のヘッダコメントで MIT を確認、コピー時に `// Source: zed-industries/zed crates/grammars/src/python/highlights.scm (MIT)` を冒頭に追加
