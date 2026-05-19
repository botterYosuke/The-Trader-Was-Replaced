# Strategy Editor を Monaco-grade に格上げする (Zed 参照)

## Context

`src/ui/strategy_editor.rs` (1254 行) は現状、bevy_cosmic_edit ベースの**単色テキストエディタ**にすぎない。基本機能 (undo/redo / 自動保存 / フラグメント管理 / multi-spawn / レイアウト永続化) は完成しているが、**ソースコードエディタとしての視認性・編集体験**が欠けている — syntax highlight も行番号もスクロールバーも Find も無い。Tab は cosmic_edit の input ハンドラが `Key::Character` でない logical key を無視するため何も起きない (= 入りも focus 遷移もしない、無音で吸われる)。

ユーザは「Zed を参考に高級ソースコードエディタに変えたい」。スコープは **Monaco-grade** で合意済 (LSP・診断・command palette・マルチカーソルは含めず、syntax / gutter / scrollbar / find&replace / auto-indent / Tab→spaces / bracket match まで)。

Syntax highlight は当初 **tree-sitter-python** ベースで検討したが、設計レビューを経て **Phase A v1 は syntect (全文再トークナイズ) に変更**。Zed から学ぶべき中心は「tree-sitter を使うこと」ではなく「**syntax / search / bracket を別責務として持ち、固定順序で合成すること**」であり、Phase A v1 では syntect + Layer Composer で「Monaco-grade の見た目」とカーソルリセットしない attrs 更新基盤を最短で出す。tree-sitter は incremental が本当に必要になったら **Phase F** で着手 (ABI / `highlights.scm` / `InputEdit` / capture mapping をその時点で扱う)。

`.claude/skills/zed/SKILL.md` に「Monaco-grade Strategy Editor は実装済」と書かれているが、これは 0.15 移行時に**実態は剥がれている** (今回の作業で実装し直す)。Phase 7.2 完了時にスキル本文も同期する。

## アーキテクチャ概要

`strategy_editor.rs` を肥大化させず、**5 つの新規モジュール**に責務を分割。既存の `StrategyBuffer` (Resource、`source` は持たない — `original_path` / `cache_path` のみ) と `StrategyFragment` (Component、`source: String` + `dirty: bool`) を**唯一の source of truth** として、各モジュールは「source が変わったら派生表示を再計算」する設計。

### 既存 ECS 構造 (重要)

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
| `strategy_editor_highlight.rs` | **(Phase A v1) syntect で Python ソースを全文再トークナイズ** → 行ごとの span 列を `SyntaxSpans` Component に書き出すだけ (= attrs はここでは適用しない)。**bracket match のスキャン**も同居し `BracketSpans` Component に書き出す | `crates/syntax_theme/src/syntax_theme.rs` (token kind → 色マップ), `crates/editor/src/highlight_matching_bracket.rs` (innermost bracket スキャン) |
| `strategy_editor_compose.rs` (**新規**) | 3 つの span source (`SyntaxSpans` / `FindMatchSpans` / `BracketSpans`) を**固定順序で合成**して各行の `AttrsList` を組み立て、`BufferLine::set_attrs_list` に適用 → `set_redraw(true)` 明示。**ここが唯一 `set_attrs_list` を呼ぶ場所**。Zed の HighlightKey 別レイヤー設計 → 描画時合成パターンを翻訳 | `crates/editor/src/display_map/custom_highlights.rs` (per-key layer + sort/merge) |
| `strategy_editor_gutter.rs` | エディタ左に独立 cosmic_text `Buffer` をもう 1 つ持つ Sprite を配置、行番号文字列を同じ Metrics で描画 | `crates/editor/src/element.rs` (paint_gutter / layout_gutter) |
| `strategy_editor_scrollbar.rs` | エディタ右に `Sprite` で thumb を描画。Pointer<Drag> で scroll を変更 | `crates/editor/src/scroll.rs` (ScrollAnchor + offset), `scroll/autoscroll.rs` (strategy enum) |
| `strategy_editor_input.rs` | Tab → 4 spaces, Enter → 前行インデント継承, 括弧キーで自動閉じ | `crates/editor/src/editor.rs::tab`, `editor::newline`, `crates/language/src/language.rs::indent_size_for_line` |
| `strategy_editor_find.rs` | 世界空間の小型パネル (`Sprite` + `CosmicEditBuffer` × 2 = query / replacement) を `spawn_floating_window` ヘルパで配置、マッチ位置を `FindMatchSpans` Component に書き出すだけ (attrs 適用は compose に委譲)、Enter/F3 で次へ。**Find editor には専用マーカー `FindQueryEditor` / `FindReplacementEditor` を付け、`StrategyEditorContent` は絶対に付けない** | `crates/search/src/buffer_search.rs` (BufferSearchBar の query/replacement editor 分離) |

### Highlight Layer Composer 設計 (Phase A の中核)

Zed の中心的な教訓は「**tree-sitter を使うこと**」ではなく「**syntax / search / bracket を別責務として持ち、最後に固定順序で合成すること**」。本プランも同パターンを採用し、Phase A v1 では実装コストの低い **syntect + 関数ベース composer** で固める。tree-sitter は v2 / Phase F に降格 (理由は後述「Phase A v1 と v2 の境界」)。

**3 つの span source (Component、editor entity に付与)**:

```rust
/// syntect が出した行ごとの span (背景色なし、forground のみ)
#[derive(Component, Default)]
pub struct SyntaxSpans {
    pub lines: Vec<Vec<SpanStyle>>,  // index = source 行番号
}

/// Find マッチ位置 (current_match は別色で塗る用に分離)
#[derive(Component, Default)]
pub struct FindMatchSpans {
    pub matches: Vec<MatchSpan>,
    pub current_idx: Option<usize>,
    /// 前回 compute 時のマッチ行集合。クエリ変更/クリア時に旧マッチ行を
    /// apply_highlight_layers_system の dirty 集合に入れて base 色へ戻すために必須。
    /// compute_find_match_spans_system が再計算前に
    /// `prev_match_lines = matches.iter().map(|m| m.line).collect()` で保存する。
    pub prev_match_lines: Vec<usize>,
}

/// bracket match — 通常 2 要素 (opener / closer)、別行にまたがる
#[derive(Component, Default)]
pub struct BracketSpans {
    pub pair: Option<[(usize /*line*/, std::ops::Range<usize>); 2]>,
}

pub struct SpanStyle {
    pub byte_range: std::ops::Range<usize>,  // line 内の byte offset
    pub fg: Option<cosmic_text::Color>,
    // ⚠️ 背景色フィールドは Phase A v1 では持たない。
    // cosmic_text::Attrs に背景色 API が無いため AttrsList 経由では塗れない。
    // Find マッチは foreground を FIND_MATCH_FG に変えるだけで視認性を担保する。
    // 本当に「黄色背景」が必要になったら Phase E v2 で別 Sprite/Text overlay layer
    // を追加 (editor の Transform に合わせて match range の rect を描画) で対応する。
}
```

**Compose 関数 (純粋関数、ユニットテスト可能)**:

```rust
pub fn compose_attrs_for_line(
    base: cosmic_text::Attrs<'_>,
    line_text: &str,
    syntax: &[SpanStyle],
    find: &[SpanStyle],
    current_find: Option<&SpanStyle>,
    bracket: &[SpanStyle],
) -> cosmic_text::AttrsList {
    let mut list = cosmic_text::AttrsList::new(base);
    // 固定順序: default → syntax → find → current find → bracket
    for span in syntax { list.add_span(span.byte_range.clone(), apply(base, span)); }
    for span in find { list.add_span(span.byte_range.clone(), apply(base, span)); }
    if let Some(span) = current_find { list.add_span(span.byte_range.clone(), apply(base, span)); }
    for span in bracket { list.add_span(span.byte_range.clone(), apply(base, span)); }
    list
}
```

**Compose system (新規 `apply_highlight_layers_system`、唯一 `set_attrs_list` を呼ぶ場所)**:

- 各 editor entity を走査
- `Changed<SyntaxSpans>` または `Changed<FindMatchSpans>` または `Changed<BracketSpans>` のいずれかが立ったフレームでのみ実行
- 変化があった span source から影響行を集合に入れる (`HashSet<usize>`):
  - `SyntaxSpans` が変化したら全行 dirty
  - `FindMatchSpans` が変化したら旧 + 新マッチ行の和集合
  - `BracketSpans` が変化したら旧 + 新 pair 行 (最大 4 行)
- dirty 行だけ `compose_attrs_for_line` で再生成 → `buffer.lines[i].set_attrs_list(...)` → 最後に `buffer.set_redraw(true)` + editor 側があれば `editor.set_redraw(true)`

**bracket cleanup の事故防止**:

旧計画の「対象 byte range にだけ syntax 色を再上塗り」はミニ composer であり、syntax と bracket のクリア順を間違えると色が消える。composer 経由なら **bracket pair が動いた / 消えた瞬間に該当行を `compose_attrs_for_line` で再生成するだけ**で正しい色に戻る (前回範囲を覚えて部分復元する必要がない)。`BracketSpans` の前回値は Local や Component の `prev_pair` フィールドで持って和集合計算にのみ使う。

### attrs 専用更新で「カーソルリセット問題」を回避 (Critical)

`CosmicEditBuffer::set_rich_text` は内部で `Buffer::set_rich_text` を呼び、buffer.lines を**全部作り直す**ため `editor.action(Action::Click ...)` を別途呼ばないとカーソルが (0,0) にリセットされる。これは既存 `strategy_editor.rs:228` のコメントが既に警告している既知の foot-gun。

→ **テキストは触らず attrs だけ差し替える**。具体的には:

1. cosmic_text `Buffer::lines: Vec<BufferLine>` を `with_buffer_mut(|b| b.lines.iter_mut())` で借り、
2. 各 `BufferLine::set_attrs_list(AttrsList)` を呼んで line ごとの spans を更新、
3. **更新が終わったら `buffer.set_redraw(true)` を明示** (重要)。`set_attrs_list` 内部は `reset_shaping` を呼ぶが、これは字形再計算のフラグであり、render system が見る Buffer 全体の `redraw` flag は立たない。[crates/bevy_cosmic_edit/src/buffer.rs:142, 160, 215, 246–256](crates/bevy_cosmic_edit/src/buffer.rs) を見ても、CosmicEditBuffer の text 系 API は全て `set_redraw(true)` を明示的に呼んでおり、`Added<CosmicEditBuffer>` 用 system まで用意されている。
4. editor 側が付いていれば `editor.set_redraw(true)` も同様に呼ぶ。

これにより:
- buffer の line 構造は不変 → カーソル位置・選択範囲は保たれる
- 表示色のみ変わる → 1 フレームで再描画される

**`set_rich_text` / `with_rich_text` で空 spans を渡して seed を捨てない** (Critical)。既存 `spawn_strategy_editor_panel` の `with_text(font_system, &seed, default_attrs)` 経路を**そのまま維持**し、初回色付けは `Added<StrategyFragment>` フィルタで `compute_syntax_spans_system` を 1 度だけ走らせ、次フレームで `apply_highlight_layers_system` が attrs を組み立てる。空 `with_rich_text([("", attrs)], attrs)` に置換すると seed が消える (set_attrs_list は attrs だけ差し替えるため、テキストは挿入されない)。

### システム実行順序 (極めて重要)

**既存 4-system チェーン ([src/ui/mod.rs:230-237](src/ui/mod.rs)) は絶対に崩さず、その末尾に新規 highlight 系を後置する**:

```
[既存・順序固定]
1. sync_editor_to_strategy_buffer_system   (CosmicTextChanged 駆動で fragment.source 更新)
2. undo_redo_system                        (Ctrl+Z/Y で record.undo/redo)
3. apply_pending_app_edits_system          (history.pending を drain して ECS 反映)
4. apply_strategy_snapshot_restore_system  (PendingStrategySnapshotRestore を fragment へ復元)
5. sync_strategy_buffer_to_editor_system   (UndoRedoApplied 駆動で editor.set_text)

[新規・5 の後に後置 — span 計算 → 合成適用の 2 段]
6.  sync_find_editors_to_state_system      (Find editor の CosmicTextChanged → FindReplaceState.query/replacement)
7a. compute_syntax_spans_system            (Changed<StrategyFragment> 駆動、syntect でトークナイズ → SyntaxSpans)
7b. compute_find_match_spans_system        (Changed<FindReplaceState> / Changed<StrategyFragment> 駆動 → FindMatchSpans)
7c. compute_bracket_spans_system           (cursor 移動駆動 → BracketSpans)
8.  apply_highlight_layers_system          (★ ここだけが set_attrs_list を呼ぶ。3 つの Span Component が
                                            いずれか Changed の editor について、影響行のみ compose_attrs_for_line で再生成)
```

なぜこの分割か:
- 既存 1-5 は **Undo/Redo パスを `PendingStrategySnapshotRestore` → `UndoRedoApplied` 経由で動かす**ため順序固定 (4→5 で snapshot が editor まで届く)。新規をその前に挟むと undo 直後の再ハイライトが 1 フレーム遅延する。
- `sync_strategy_buffer_to_editor_system:262` は `set_text(..., Attrs::new())` で**全行 attrs をデフォルト色にリセット**する (Undo/Redo 直後のみ)。`apply_highlight_layers_system` (8) を後に置くことで、set_text で色が消えても同じフレーム内で再合成される (1 フレーム白フラッシュなし)。
- **7a/7b/7c は読み取り (`fragment.source` / `FindReplaceState` / cursor) と書き込み (`SyntaxSpans` / `FindMatchSpans` / `BracketSpans`) が独立**なので、Bevy のスケジューラが並列実行できる (順序指定は最小限、`before(apply_highlight_layers_system)` のみ)。
- Find editor 同期 (6) は 7b の前。Find 入力で `FindReplaceState.query` が変わった瞬間に 7b がマッチ位置を再計算する。

⚠️ **`chain()` 全段連結はしない**。1-5 は既存 `.after(...)` 個別指定で接続済 (mod.rs:230-237 参照)、新規 6-8 は以下で接続:
- `6` は `.after(sync_strategy_buffer_to_editor_system)`
- `7a/7b/7c` はそれぞれ `.after(sync_strategy_buffer_to_editor_system).after(sync_find_editors_to_state_system).before(apply_highlight_layers_system)`
- `8` は `.after(7a, 7b, 7c)`

各 compute_*_spans system は **`set_attrs_list` を呼ばない**。Span Component を書き換えるだけ。これにより「Find クリア時の前回範囲復元」のような状態跨ぎロジックが不要になり、`apply_highlight_layers_system` が dirty 行を `compose_attrs_for_line` で**毎回再生成**するだけで正しい色に収束する (zed スキル Caveat 6 の重ね順問題は composer の固定順序で自動解決)。

### Changed フィルタ駆動

- highlight 再計算は `Changed<StrategyFragment>` (Bevy フィルタ) で発火
- `sync_editor_to_strategy_buffer_system` (既存 286 行付近) と Undo/Redo (`PendingStrategySnapshotRestore` 経路、`editor_history.rs:359`) の**両方**で `fragment.source` を書き換えれば自動的に Changed が立つ (明示的な dirty 立てとは別経路)
- 入力中の毎フレーム再ハイライトはコスト測定後に判断: tree-sitter Python の incremental parse は 1KB 程度のソースで数 ms。300 行超で重い場合のみ 100ms デバウンス (`Local<Timer>`) を後付け

### cosmic_edit との接続点

- 初回は既存 `with_text(font_system, &seed, default_attrs)` を**維持**して seed を buffer 構築と同時に注入する (空 `with_rich_text` に置換しない、Caveat 25 参照)。初期色付けは `Added<StrategyFragment>` フィルタで `compute_syntax_spans_system` が 1 度走り、次フレームで `apply_highlight_layers_system` が attrs を適用する
- 以降の attrs 更新はヘルパ `fn for_each_buffer(entity, |buffer: &mut cosmic_text::Buffer| { ... })` に集約する。実装は:
  - `CosmicEditor` がエンティティに付いていれば `editor.with_buffer_mut(|b| f(b))` を呼ぶ (focused = render はこちらを見る、`render.rs:88` 参照)
  - `CosmicEditBuffer` 単独なら `f(&mut buffer.0)` を呼ぶ (unfocused = render はこちらを見る)
  - 両方付いている場合は editor 側のみで足りる (CosmicEditBuffer は editor がフォーカスを失ったときの復元元になっていて、focus 切替の瞬間に editor から書き戻される設計なので、焦らず editor だけ更新する)
- 実装簡略化のため Phase A v1 では **editor 側があれば editor のみ、無ければ CosmicEditBuffer 側のみ** を更新する (両更新は不要、上記の理由でロスしない)
- ヘルパ末尾で必ず `buffer.set_redraw(true)` ＋ editor 側があれば `editor.set_redraw(true)` を呼ぶ

## 実装フェーズ (5 段階、各 1 PR 想定)

依存関係: A → B,C,D,E は並行可能。Find (E) が最も独立。Tab/Enter (C) は input fork に触る可能性あり。

### Phase A: syntect syntax highlight + Layer Composer (最重要)

**方針**: tree-sitter は ABI / scm asset / capture mapping / incremental input edit / byte-line 変換と未確定要素が多い。Phase A v1 では **syntect で全文再トークナイズ → composer で合成** に統一し、「Monaco-grade の見た目」と「カーソルリセット問題が起きない attrs 更新基盤」を最短で出す。tree-sitter は Phase A v2 / Phase F に降格 (incremental が本当に必要になってから扱う)。

**Cargo.toml に追加:**
```toml
syntect = { version = "5", default-features = false, features = ["default-fancy"] }
# default-fancy = fancy-regex バックエンド。Windows で onig (C 依存) を避けるため必須
```

**新規 `src/ui/strategy_editor_highlight.rs`:**

syntect の格納形式 (通常の `Resource` で OK — syntect の `SyntaxSet` / `ThemeSet` は `Send + Sync`):

```rust
#[derive(Resource)]
pub struct SyntectHighlighter {
    pub syntax_set: syntect::parsing::SyntaxSet,
    pub theme: syntect::highlighting::Theme,
    pub python_syntax: syntect::parsing::SyntaxReference,  // load_defaults_newlines から find_syntax_by_extension("py")
}

fn init_syntect_highlighter(mut commands: Commands) {
    // ⚠️ load_defaults_newlines は数十〜百 ms。Startup で 1 回だけ実行
    let syntax_set = syntect::parsing::SyntaxSet::load_defaults_newlines();
    let theme_set = syntect::highlighting::ThemeSet::load_defaults();
    let theme = theme_set.themes["base16-mocha.dark"].clone();  // Dracula 風で読みやすい既定
    let python_syntax = syntax_set
        .find_syntax_by_extension("py")
        .expect("syntect default set includes python")
        .clone();
    commands.insert_resource(SyntectHighlighter { syntax_set, theme, python_syntax });
}
```

(syntect の `Theme` には既に foreground / scope-based 色マップが内包されているので、Phase A v1 では `components.rs` に SYNTAX_* 色定数を追加する必要はない。後で独自 Dracula 風テーマに切り替えたくなったら `Theme::from_settings` で .tmTheme を読む)

**Span source Component (新規 3 つ)**:

```rust
#[derive(Component, Default)]
pub struct SyntaxSpans { pub lines: Vec<Vec<SpanStyle>> }

#[derive(Component, Default)]
pub struct FindMatchSpans {
    pub matches: Vec<MatchSpan>,
    pub current_idx: Option<usize>,
    pub prev_match_lines: Vec<usize>,  // 旧マッチ行 — クリア時に dirty 行へ含める
}

#[derive(Component, Default)]
pub struct BracketSpans {
    pub pair: Option<[(usize, std::ops::Range<usize>); 2]>,
    pub prev_pair: Option<[(usize, std::ops::Range<usize>); 2]>,  // 和集合計算用
}
```

editor entity spawn 時 (`spawn_strategy_editor_panel`) にこの 3 つの Component を `..default()` で挿入する。

**`compute_syntax_spans_system` (syntect トークナイズ → SyntaxSpans 書き込み)**:

```rust
fn compute_syntax_spans_system(
    highlighter: Res<SyntectHighlighter>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), (With<WindowRoot>, Changed<StrategyFragment>)>,
    mut editor_q: Query<(&StrategyEditorId, &mut SyntaxSpans), With<StrategyEditorContent>>,
) {
    for (frag_id, fragment) in &fragments_q {
        let Some((_, mut spans)) = editor_q.iter_mut()
            .find(|(id, _)| id.region_key == frag_id.region_key)
        else { continue };

        let mut h = syntect::easy::HighlightLines::new(&highlighter.python_syntax, &highlighter.theme);
        let mut new_lines: Vec<Vec<SpanStyle>> = Vec::with_capacity(fragment.source.lines().count());
        for line in syntect::util::LinesWithEndings::from(&fragment.source) {
            let ranges = h.highlight_line(line, &highlighter.syntax_set)
                .unwrap_or_default();  // 失敗時は空 spans (= default color にフォールバック)
            new_lines.push(convert_syntect_ranges_to_spans(line, ranges));
        }
        spans.lines = new_lines;  // DerefMut → Changed<SyntaxSpans> が立つ
    }
}

fn convert_syntect_ranges_to_spans(
    line: &str,
    ranges: Vec<(syntect::highlighting::Style, &str)>,
) -> Vec<SpanStyle> {
    let mut out = Vec::with_capacity(ranges.len());
    let mut byte_offset = 0;
    for (style, text) in ranges {
        let len = text.len();
        let fg = style.foreground;
        out.push(SpanStyle {
            byte_range: byte_offset..(byte_offset + len),
            fg: Some(cosmic_text::Color::rgba(fg.r, fg.g, fg.b, fg.a)),
            bg: None,
        });
        byte_offset += len;
    }
    out
}
```

**`compute_bracket_spans_system` (bracket スキャン → BracketSpans 書き込み)**:

毎フレーム実行 (`Local<HashMap<Entity, cosmic_text::Cursor>>` で前回 cursor を持ち、変化なしならスキップ)。`CosmicTextChanged` だけでは矢印キー/クリックでのカーソル移動を検出できない (`input.rs:518` の `is_edit` 制約)。focused editor を `Query<(Entity, &CosmicEditor), With<StrategyEditorContent>>` で取り `FocusedWidget.0` と一致したものだけ処理。

- カーソル前後 1 文字で `(){}[]` の対応を innermost スキャン (AST 不要、最大 4096 文字打ち切り)
- マッチが見つかったら `BracketSpans.prev_pair = pair.take()` で前回値を退避してから `pair = Some([(line, range), (line, range)])` を書く
- マッチなしなら `prev_pair = pair.take(); pair = None`
- 8 で `prev_pair` と `pair` の union を dirty 行とする

**Compose 関数 (純粋関数、新規 `src/ui/strategy_editor_compose.rs`)**:

```rust
pub fn compose_attrs_for_line(
    base: cosmic_text::Attrs<'_>,
    syntax: &[SpanStyle],
    find: &[SpanStyle],
    current_find: Option<&SpanStyle>,
    bracket: &[SpanStyle],
) -> cosmic_text::AttrsList {
    let mut list = cosmic_text::AttrsList::new(base);
    // 順序固定: default → syntax → find → current find → bracket
    for s in syntax { list.add_span(s.byte_range.clone(), apply_span(base, s)); }
    for s in find { list.add_span(s.byte_range.clone(), apply_span(base, s)); }
    if let Some(s) = current_find { list.add_span(s.byte_range.clone(), apply_span(base, s)); }
    for s in bracket { list.add_span(s.byte_range.clone(), apply_span(base, s)); }
    list
}

fn apply_span<'a>(base: cosmic_text::Attrs<'a>, span: &SpanStyle) -> cosmic_text::Attrs<'a> {
    let mut a = base;
    if let Some(fg) = span.fg { a = a.color(fg); }
    a
}
// ⚠️ Phase A v1 では foreground 色のみ。Find マッチは FIND_MATCH_FG、current match は
// FIND_CURRENT_MATCH_FG、bracket match は BRACKET_MATCH_FG を fg に詰めて区別する。
// 背景色 (黄色ハイライト風) が必要なら Phase E v2 で別 Sprite overlay layer を追加。
```

これを `#[cfg(test)]` で 「syntax 1 span + find 1 span + bracket 1 span が重なったとき bracket 色が勝つ」テストで固める。

**`apply_highlight_layers_system` (★ 唯一 `set_attrs_list` を呼ぶ場所)**:

```rust
fn apply_highlight_layers_system(
    mut editor_q: Query<
        (
            &mut CosmicEditBuffer,
            Option<&mut CosmicEditor>,
            Ref<SyntaxSpans>,
            Ref<FindMatchSpans>,
            Ref<BracketSpans>,
            &DefaultAttrs,
        ),
        With<StrategyEditorContent>,
    >,
) {
    for (mut buffer, editor_opt, syntax, find, bracket, default_attrs) in &mut editor_q {
        // どれか 1 つでも Changed なら処理
        if !syntax.is_changed() && !find.is_changed() && !bracket.is_changed() { continue; }

        // dirty 行を集める
        let mut dirty: HashSet<usize> = HashSet::new();
        if syntax.is_changed() {
            // SyntaxSpans が変化 = ソース変化 = 全行 dirty (Phase A v1)
            for i in 0..syntax.lines.len() { dirty.insert(i); }
        }
        if find.is_changed() {
            for m in &find.matches { dirty.insert(m.line); }
            // 旧マッチ行も dirty に入れる。query クリア時 (matches が空に) に旧色を
            // base に戻すために必須。compute_find_match_spans_system が再計算前に
            // prev_match_lines を更新している前提。
            for &line in &find.prev_match_lines { dirty.insert(line); }
        }
        if bracket.is_changed() {
            if let Some(prev) = &bracket.prev_pair {
                dirty.insert(prev[0].0); dirty.insert(prev[1].0);
            }
            if let Some(cur) = &bracket.pair {
                dirty.insert(cur[0].0); dirty.insert(cur[1].0);
            }
        }

        // dirty 行を composer で再生成
        let base = default_attrs.0.as_attrs();
        // editor 側があれば editor のみ更新、無ければ buffer のみ
        let apply = |b: &mut cosmic_text::Buffer| {
            for &i in &dirty {
                let Some(line) = b.lines.get_mut(i) else { continue };
                let syntax_spans = syntax.lines.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
                let find_spans: Vec<SpanStyle> = find.matches.iter()
                    .enumerate()
                    .filter(|(idx, m)| m.line == i && Some(*idx) != find.current_idx)
                    .map(|(_, m)| span_from_match(m, false)).collect();
                let current_find = find.current_idx
                    .and_then(|idx| find.matches.get(idx))
                    .filter(|m| m.line == i)
                    .map(|m| span_from_match(m, true));
                let bracket_spans: Vec<SpanStyle> = bracket.pair.iter()
                    .flat_map(|pair| pair.iter())
                    .filter(|(line, _)| *line == i)
                    .map(|(_, range)| SpanStyle {
                        byte_range: range.clone(),
                        fg: Some(BRACKET_MATCH_FG),
                    }).collect();
                let attrs_list = compose_attrs_for_line(
                    base, syntax_spans, &find_spans, current_find.as_ref(), &bracket_spans,
                );
                line.set_attrs_list(attrs_list);
            }
            b.set_redraw(true);
        };

        if let Some(mut editor) = editor_opt {
            editor.with_buffer_mut(|b| apply(b));
            editor.set_redraw(true);
        } else {
            apply(&mut buffer.0);
        }
    }
}
```

(`FindMatchSpans.prev_match_lines` の更新は `compute_find_match_spans_system` 冒頭で `prev_match_lines = matches.iter().map(|m| m.line).collect()` と保存してから `matches` を再計算する。bracket と同じパターン: 「前回値を退避 → 新値を計算 → apply 側が両方を dirty に入れる」。)

**Phase A v1 と v2/Phase F の境界:**

- **v1 (本フェーズ)**: syntect で全文再トークナイズ + composer。Python 1KB で数 ms、500 行でも体感問題なし (`Changed<StrategyFragment>` 駆動なのでアイドル時は 0 コスト)。
- **v2 / Phase F (将来、必要になったら)**: tree-sitter-python に置換。`SyntaxSpans` を吐く部分だけ差し替えれば composer 以降は無変更。incremental parse、ABI、`highlights.scm` の取得・ライセンス・出典コメント (`;` 開始)、Query capture → 色マッピング、`InputEdit` 設計はこの時点でまとめて扱う。**性能要件 (500 行以上で目視ラグ) が観測されない限り Phase F は着手しない**。

**修正: `src/ui/strategy_editor.rs`**
- ⚠️ **初期 `with_text` の置換は seed を捨てないように**: 現行 `src/ui/strategy_editor.rs:154-162` は `with_text(font_system, &seed, default_attrs)` で seed を buffer 構築と同時に注入している。空 `with_rich_text([("", attrs)], attrs)` に置換すると **seed が消える** (Phase A v1 の composer は `set_attrs_list` で attrs だけ差し替えるため、テキストは挿入されない)。以下の二択で対応:
  - (推奨) **`with_text(font_system, &seed, default_attrs)` を維持**し、初期色は `Added<StrategyFragment>` フィルタを `compute_syntax_spans_system` に追加して 1 度だけ走らせる (= 次フレームで `SyntaxSpans` が埋まり、`apply_highlight_layers_system` が attrs を適用)
  - (代替) `with_rich_text(&[(seed.as_str(), default_attrs)], default_attrs)` で **seed を rich_text の唯一の span として渡す** (空文字列ではなく seed 本文を渡す)。以後の attrs 更新は composer 経由で安全
- `sync_strategy_buffer_to_editor_system:262` 周辺の `set_text` 経路は変更しない (set_text 後は Changed<StrategyFragment> が立つので compute_syntax_spans_system が次フレームで再トークナイズし、composer が再合成する)
- システム登録 (`src/main.rs` か Plugin 集約点) で **既存 4-system チェーンを保ったまま新規 5 つを後置** (具体的な接続は後述「触るファイル一覧」の `src/main.rs` 節と完全一致させること):

  ```rust
  // 既存は src/ui/mod.rs:230-237 にあるのでそのまま温存
  app.add_systems(Startup, init_syntect_highlighter);
  app.add_systems(
      Update,
      (
          sync_find_editors_to_state_system
              .after(sync_strategy_buffer_to_editor_system),
          compute_syntax_spans_system
              .after(sync_strategy_buffer_to_editor_system)
              .before(apply_highlight_layers_system),
          compute_find_match_spans_system
              .after(sync_find_editors_to_state_system)
              .before(apply_highlight_layers_system),
          compute_bracket_spans_system
              .after(sync_strategy_buffer_to_editor_system)
              .before(apply_highlight_layers_system),
          apply_highlight_layers_system,  // ★ 唯一 set_attrs_list を呼ぶ
      ),
  );
  ```

  `chain()` は使わない (compute_*_spans は読み書き対象が独立なのでスケジューラに並列化させる)。`apply_highlight_layers_system` は 3 つの `compute_*_spans_system` を全部 before で指定するので自然に最後に置かれる。**既存の `sync_editor_to_strategy_buffer → undo_redo → apply_pending → apply_strategy_snapshot_restore → sync_strategy_buffer_to_editor` を計画書から削除・並べ替えしない**。`add_systems` タプル 20 上限に注意 (Phase A〜E で 10+ system を追加するので、既存登録数次第で他のタプルから 1〜2 個外して別 `add_systems` 呼び出しに分割)。

### Phase B: 行番号ガター + スクロールバー

**新規 `src/ui/strategy_editor_gutter.rs`:**

- `LineNumberGutter` Component — エディタ左 36px (font_size 14 で 5 桁分 + padding) に `Sprite` (背景) + **もう 1 つの独立 `CosmicEditBuffer`** (read-only、`ReadOnly` component 付加) を子として配置
  - 別 cosmic_text Buffer を持つことで、Metrics をエディタと完全一致させ、行の高さズレを根本的に排除
  - 共通定数 `EDITOR_METRICS: Metrics = Metrics::new(14.0, 18.0)` を `strategy_editor.rs` に置き、ガター/エディタ/find 全部で共有
- `update_gutter_text_system` — `Changed<StrategyFragment>` で `(1..=line_count).map(|i| format!("{i:>4}")).join("\n")` を gutter buffer に `set_text`、最後に `set_redraw(true)` を明示
- スクロール追従: エディタ側の `editor.with_buffer(|b| { (b.scroll().line, b.scroll().vertical) })` を読み、ガター buffer の `set_scroll` に同じ値を入れる (line + vertical 両方コピー必須)
- **wrap モード**: エディタを `cosmic_text::Wrap::None` に固定する。`Buffer::set_wrap(&mut self, font_system: &mut FontSystem, wrap: Wrap)` は `FontSystem` 必須なので、startup 時に `Res<CosmicFontSystem>` から借りて `editor_buffer.0.set_wrap(&mut font_system.0, Wrap::None)` を 1 回呼ぶ (gutter 用の Buffer にも同様)。これで「source 行 == layout 行」になり、ガター行番号と scrollbar の line 数が一致する。長い行は横スクロール (cosmic_edit が `XOffset` で対応)。
  
  ⚠️ **widget 側の wrap 上書きに注意**: `TextEdit2d` の render system は `Sprite.custom_size` から buffer 幅を計算して `Buffer::set_size` 経由で wrap 値を**間接的に上書きするコードパス**を持つ場合がある (cosmic_edit のバージョンに依存)。Phase B 完了時の verification で「`Sprite.custom_size` を Phase B のレイアウト調整に合わせて変更した後でも、長い行が折り返されず横にはみ出すこと (= `Wrap::None` が維持されていること)」を目視確認する。維持されていない場合は `set_wrap(Wrap::None)` を毎フレーム呼ぶ system を 1 つ足す (1 回 / 1 editor の軽量呼び出し)。

**新規 `src/ui/strategy_editor_scrollbar.rs`:**

- `EditorScrollThumb { target_editor: Entity }` Component — エディタ右 8px に `Sprite`。`target_editor` フィールドで「どのエディタを操作する thumb か」を保持する (multi-spawn で複数 thumb が並ぶため必須)
- thumb サイズ: `thumb_h = (viewport_lines / total_lines).clamp(0.05, 1.0) * scrollbar_h`
- thumb 位置: `(scroll.line as f32 / (total_lines - viewport_lines).max(1) as f32) * (scrollbar_h - thumb_h)`
  - `Scroll::vertical` (line 内 pixel 微調整) は thumb 位置には反映しない (微小なので無視)
- `Pointer<Drag>` observer で thumb を縦ドラッグ → `trigger.entity()` で thumb entity を取得 (⚠️ **Bevy 0.15 は `trigger.entity()`**、`trigger.target()` は 0.16+ で導入された rename API。`src/ui/floating_window.rs:55-63` の既存 observer と揃える) → そこから `EditorScrollThumb::target_editor` を引いて対象エディタを特定 → drag.delta.y を line に逆換算 → `editor.with_buffer_mut(|b| b.set_scroll(Scroll { line: new_line, vertical: 0.0, horizontal: 0.0 }))` → `editor.set_redraw(true)` を明示
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

⚠️ **Critical: カスタム編集は `CosmicTextChanged` を手動で発行する必要がある**

`crates/bevy_cosmic_edit/src/input.rs:475-521` を見ると、`CosmicTextChanged` イベントは **cosmic_edit の input system 内で `is_edit = true` のときだけ** 発火する (line 518)。`is_edit` は char 入力か Enter で立つが、Phase C では:
- Tab → `keys.reset(KeyCode::Tab)` 相当で cosmic_edit の Tab パスを完全にバイパス
- Enter → `keys.reset(KeyCode::Enter)` で cosmic_edit の Enter 処理を**抑止**
- 括弧 closer → 我々が `editor.action(Action::Insert(closer))` を直接呼ぶ (cosmic_edit は opener しか挿入しない)

→ **これらのカスタム編集経路では `CosmicTextChanged` が発火しない** ため、`sync_editor_to_strategy_buffer_system` ([line 283-328](src/ui/strategy_editor.rs)) に届かず、`fragment.source` 更新・autosave・undo・再 highlight すべて空振りする。**各カスタム編集 system の末尾で、編集が発生した場合のみ手動で `CosmicTextChanged` を `EventWriter` 経由で送る**:

```rust
fn tab_input_system(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, &mut CosmicEditor), With<StrategyEditorContent>>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut evw_changed: EventWriter<CosmicTextChanged>,
) {
    if !keys.just_pressed(KeyCode::Tab) { return; }
    let Some(focus_entity) = focused.0 else { return };
    let Ok((entity, mut editor)) = editor_q.get_mut(focus_entity) else { return };
    for _ in 0..4 {
        editor.action(&mut font_system.0, Action::Insert(' '));
    }
    keys.reset(KeyCode::Tab);  // cosmic_edit が将来 Tab を扱う場合に備え
    let new_text = editor.with_buffer_mut(|b| b.get_text());
    evw_changed.send(CosmicTextChanged((entity, new_text)));
}
```

`enter_autoindent_system` と `bracket_autoclose_system` も同様、**編集発生時のみ `EventWriter<CosmicTextChanged>` で送る**。同じ entity に対して同じ全文を 1 フレーム内に複数回送っても、`sync_editor_to_strategy_buffer_system:316` の `if fragment.source == *new_text { continue; }` で短絡されるので重複は安全。

- `tab_input_system`:
  - `ResMut<ButtonInput<KeyCode>>` で `KeyCode::Tab` just_pressed をチェック、`FocusedWidget == strategy_editor_entity` で発火
  - `editor.action(Action::Insert(' '))` を 4 回呼ぶ (cosmic_text の Editor API は char 単位、`insert_string` は `Editor::borrow_with(font_system)` 経由でしか呼べないため char ループの方が簡潔)
  - 起こりうる二重発火を防ぐため `before(bevy_cosmic_edit::input::InputSet)` を必ず付ける (cosmic_edit が Tab を Character 扱いしないとはいえ将来変更に備える)
  - **末尾で `CosmicTextChanged` を手動 send (上記コード参照)**
- `enter_autoindent_system`:
  - `ResMut<ButtonInput<KeyCode>>` + `FocusedWidget` 一致 で `KeyCode::Enter` just_pressed をチェック
  - 前行の `&fragment.source` から `\n` 直前の行を取り出し、`len() - trim_start().len()` でインデント幅を抽出
  - `editor.action(Action::Insert('\n'))` → `editor.action(Action::Insert(' '))` を indent 幅ぶん繰り返す
  - **`keys.reset(KeyCode::Enter)`** を呼んで cosmic_edit の Enter 処理を抑止
  - **末尾で `CosmicTextChanged` を手動 send** — これが無いと改行が autosave / undo / highlight に伝播しない
  - `before(bevy_cosmic_edit::input::InputSet)` 必須
- `bracket_autoclose_system`:
  - 入力文字が `(`, `[`, `{`, `"`, `'` のとき、**かつ次の文字が同じ closer (`)`, `]`, `}`, `"`, `'`) でないとき** のみ closer を後置 (`Action::Insert(closer)` → `Action::Motion(Motion::Left)`)
  - 選択範囲がある場合は「選択を囲む」(将来拡張、Phase C v1 では選択ありなら autoclose しないでスキップ)
  - コメント/文字列の中での autoclose 抑止は v2 (tree-sitter Tree から「いまカーソルがどの node の中か」を取れば判別できる、まずは無し)
  - **タイミング**: cosmic_edit 自身が opener (`(` 等) を `EventReader<KeyboardInput>` で読み挿入するので、我々は `.after(bevy_cosmic_edit::input::InputSet)` で動き、**`Events::clear()` は呼ばない** (cosmic_edit の opener 挿入を奪わない)。我々のシステムは `EventReader<KeyboardInput>` を**読むだけ** (clear せず) で文字種を判定し、cosmic_edit が opener を挿入した直後の cursor 位置に closer を後置する
  - **closer 挿入が発生したフレームのみ `CosmicTextChanged` を手動 send** — cosmic_edit が opener 挿入時に送ったイベントには closer 分が含まれないため、closer 後の全文を改めて配信する必要がある

### Phase D: Bracket match — Phase A に含めて完了

(設計上 `strategy_editor_highlight.rs` の `compute_bracket_spans_system` + `strategy_editor_compose.rs` の `apply_highlight_layers_system` で吸収したので Phase D は省略。bracket 色の上書きは composer の固定順序で保証される。)

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

- **Find エディタの専用マーカー:**

  ```rust
  #[derive(Component)]
  pub struct FindQueryEditor;

  #[derive(Component)]
  pub struct FindReplacementEditor;
  ```

  Find パネルの 2 つの `CosmicEditBuffer` には **`StrategyEditorContent` を絶対に付けない**。代わりに上記の専用マーカーを付ける。

  なぜ重要か: 既存 `sync_editor_to_strategy_buffer_system` ([line 283–328](src/ui/strategy_editor.rs)) は `editor_q: Query<&StrategyEditorId, With<StrategyEditorContent>>` で絞っているので、`StrategyEditorContent` を付けないだけで自動的に Find editor の `CosmicTextChanged` は無視される。逆に、Navigator が雛形コピペで誤って `StrategyEditorContent` を付けると、Find への入力 1 文字ごとに対象 Strategy Editor の `fragment.source` がそれで上書きされる事故になる。明記必須。

  highlight 系 (`compute_syntax_spans_system` / `compute_find_match_spans_system` / `compute_bracket_spans_system` / `apply_highlight_layers_system`) も同じ理由で `With<StrategyEditorContent>` フィルタを使うので、Find editor は highlight 対象外になる (= プレーンテキストで表示される、それで OK)。

- **Find パネルのライフサイクル管理 (Critical):**

  `find_replace_ui_system` を「`is_open == true` のとき毎フレーム spawn する」と書くと、**毎フレーム新しい panel entity が積み重なる**。Bevy の retained UI は明示的な despawn が無い限り消えない (Zed の `BufferSearchBar` は GPUI の View ライフサイクルで自動消滅するが、我々の Sprite ベース実装には類似機構が無い)。

  → **`FindReplaceState` に `panel_root: Option<Entity>` を持たせて 1 度だけ spawn / despawn する**:

  ```rust
  #[derive(Resource, Default)]
  pub struct FindReplaceState {
      pub query: String,
      pub replacement: String,
      pub case_sensitive: bool,
      pub matches: Vec<MatchSpan>,
      pub current: usize,
      pub is_open: bool,
      pub target_editor: Option<Entity>,
      pub panel_root: Option<Entity>,            // ★ spawn 済み panel の root entity
      pub query_editor: Option<Entity>,          // ★ FindQueryEditor を付けた child entity
      pub replacement_editor: Option<Entity>,    // ★ FindReplacementEditor を付けた child entity
  }
  ```

  `manage_find_panel_lifecycle_system` (新規、`find_replace_ui_system` から分離):
  - false→true 遷移 (`is_open && panel_root.is_none()`) で 1 回だけ `spawn_floating_window` + 2 つの child editor spawn、`panel_root` / `query_editor` / `replacement_editor` を保存
  - true→false 遷移 (`!is_open && panel_root.is_some()`) で `commands.entity(panel_root.take().unwrap()).despawn_recursive()`、`query_editor` / `replacement_editor` も `None` に
  - 親 panel_root が外から despawn された場合の検証: 各フレーム冒頭で `panel_root.and_then(|e| editor_q.get(e).ok()).is_none()` なら state を default にリセット (= 開き直しが可能になる)

- **Find editor の入力 → `FindReplaceState` の同期 (Critical):**

  Find query/replacement editor は `StrategyEditorContent` を付けないので既存 `sync_editor_to_strategy_buffer_system` は無視するが、**誰も `state.query` / `state.replacement` を更新しない**まま `find_match_recompute_system` を「`state.query` 変化で発火」と書いても永久に発火しない。

  → **`sync_find_editors_to_state_system` を新規追加** (実行順序: `sync_strategy_buffer_to_editor` の直後、`find_match_recompute_system` の前):

  ```rust
  fn sync_find_editors_to_state_system(
      mut events: EventReader<CosmicTextChanged>,
      query_q: Query<Entity, (With<FindQueryEditor>, Without<StrategyEditorContent>)>,
      replacement_q: Query<Entity, (With<FindReplacementEditor>, Without<StrategyEditorContent>)>,
      mut state: ResMut<FindReplaceState>,
  ) {
      for CosmicTextChanged((entity, new_text)) in events.read() {
          if query_q.contains(*entity) {
              if state.query != *new_text {
                  state.query = new_text.clone();  // Changed<FindReplaceState> が立つ
              }
          } else if replacement_q.contains(*entity) {
              if state.replacement != *new_text {
                  state.replacement = new_text.clone();
              }
          }
      }
  }
  ```

  この system は **history / autosave / fragment には絶対に触らない**。`Without<StrategyEditorContent>` を二重ガードとして明示する (マーカーの取り違えを compile 時に近い形で検出)。

- `find_replace_ui_system` — **bevy_egui は現状 Cargo.toml に無いので、既存 `spawn_floating_window` ヘルパで世界空間の小型パネルを `manage_find_panel_lifecycle_system` で 1 回だけ spawn する**。パネル内に `CosmicEditBuffer` × 2 (query / replacement、各 `MaxLines(1)` + 専用マーカー `FindQueryEditor` / `FindReplacementEditor`) と、行/件数表示用 `Text2d`、「Prev」「Next」「Replace」「Replace All」用の Sprite + Pointer<Click> observer 4 個を子配置。bevy_egui を導入する案も検討余地はあるが、Phase 7.2 の最短ルートとしては既存パターンの再利用を優先する
- `compute_find_match_spans_system` (旧 `find_match_recompute_system`) — `Changed<FindReplaceState>` or `Changed<StrategyFragment>` で全マッチ再計算 (plain substring match、regex は v2)。結果は対象 editor の **`FindMatchSpans` Component に書き込むだけ**で attrs には触らない。再計算冒頭で `prev_match_lines = matches.iter().map(|m| m.line).collect()` を保存してから `matches` を更新するので、`apply_highlight_layers_system` がクリア時にも旧行を dirty 集合に入れて base 色に戻せる (`FindMatchSpans::prev_match_lines` フィールド定義参照)
- **`apply_find_match_highlight_system` は廃止** — マッチ列の attrs 適用は `apply_highlight_layers_system` (composer) が責務として持つ。Find システムは「state を更新するだけ」。これで syntax / find / bracket のクリア順事故が構造的に発生しない
- `find_scroll_to_match_system` — `FindMatchSpans.current_idx` 変更で `editor.with_buffer_mut(|b| b.set_scroll(Scroll { line: match.line.saturating_sub(viewport_lines / 2), vertical: 0.0, horizontal: 0.0 }))` で対象行を画面中央へ、`editor.set_redraw(true)`
- Cmd/Ctrl+F で開く: `ResMut<ButtonInput<KeyCode>>` で modifier + KeyF を見て `is_open = true` + `target_editor = focused_widget.0` をセット (panel spawn は `manage_find_panel_lifecycle_system` が次フレーム検出してから)、**spawn 後に `FocusedWidget` を `state.query_editor` のエンティティに切り替える** (lifecycle system 末尾で transition 検出して 1 回だけ実行)
- Esc で閉じる: `is_open = false` をセット + `FocusedWidget` を `target_editor` に戻す。despawn は `manage_find_panel_lifecycle_system` が次フレームで実施

**target_editor の lifecycle (重要):**

- find が開いている状態で対象 editor の panel が閉じられる/despawn される可能性 → `find_match_recompute_system` 冒頭で `editor_q.get(target_editor).is_err()` なら `FindReplaceState::default()` でリセット
- multi-spawn 時は「最後に focus していた editor」を対象とする (グローバル単一の FindReplaceState で十分、Zed の per-pane search は将来)

## 触るファイル一覧

**新規 (6 ファイル):**
- `src/ui/strategy_editor_highlight.rs` — syntect トークナイズ → `SyntaxSpans` + bracket スキャン → `BracketSpans`
- `src/ui/strategy_editor_compose.rs` — `compose_attrs_for_line` + `apply_highlight_layers_system` (★ 唯一 set_attrs_list を呼ぶ)
- `src/ui/strategy_editor_gutter.rs`
- `src/ui/strategy_editor_scrollbar.rs`
- `src/ui/strategy_editor_input.rs`
- `src/ui/strategy_editor_find.rs`

**新規 (アセット): なし** — syntect 既定の Python 構文と base16-mocha.dark テーマを使うので、外部 .scm / .tmTheme コピーは Phase A v1 では不要 (独自配色にしたくなったら Phase A v1.5 で `assets/themes/dracula.tmTheme` を追加して `Theme::from_settings` で読む)

**修正:**
- `Cargo.toml` — `syntect = { version = "5", default-features = false, features = ["default-fancy"] }` を追加 (fancy-regex バックエンドで Windows の C 依存 onig を回避)
- `src/ui/mod.rs` — 6 モジュール宣言
- `src/main.rs` (または既存の plugin 集約点) — **既存 4-system チェーン (`src/ui/mod.rs:230-237`) は並べ替えない**、新規システムを末尾に後置:
  - **Startup**: `init_syntect_highlighter`
  - `Phase A`: `compute_syntax_spans_system` / `compute_bracket_spans_system` / `apply_highlight_layers_system`
  - `Phase B`: `update_gutter_text_system` / `sync_gutter_scroll_system` / `update_scrollbar_thumb_system`
  - `Phase C`: `tab_input_system` / `enter_autoindent_system` / `bracket_autoclose_system` (順序は `before(InputSet)` or `after(InputSet)` で個別指定)
  - `Phase E`: `manage_find_panel_lifecycle_system` / `sync_find_editors_to_state_system` / `compute_find_match_spans_system` / `find_scroll_to_match_system` (apply_find_match_highlight_system は廃止 — composer 統合済)
  - **新規 highlight 系の接続**:
    - `sync_find_editors_to_state_system.after(sync_strategy_buffer_to_editor_system)`
    - `compute_syntax_spans_system.after(sync_strategy_buffer_to_editor_system).before(apply_highlight_layers_system)`
    - `compute_find_match_spans_system.after(sync_find_editors_to_state_system).before(apply_highlight_layers_system)`
    - `compute_bracket_spans_system.after(sync_strategy_buffer_to_editor_system).before(apply_highlight_layers_system)`
    - `apply_highlight_layers_system` は前者 3 つ全部を `after` (= スケジューラが自然に最後に置く)
  - **`SyntectHighlighter` は `app.init_resource::<SyntectHighlighter>()` ではなく Startup system で `commands.insert_resource(...)`** (load_defaults_newlines が初期化コスト数十 ms なので遅延ロード)
  - **`FindReplaceState` は `app.init_resource::<FindReplaceState>()`** で登録
  - `add_systems` タプル 20 上限に注意 (現状の登録数を確認、超えたら chain で分割)
- `src/ui/strategy_editor.rs`:
  - `spawn_strategy_editor_panel` で gutter/scrollbar も spawn、Sprite サイズ計算を `EDITOR_PANEL_SIZE` / `EDITOR_TEXT_SIZE` に分離
  - **`with_text` は維持** (seed を捨てないため、Finding 2 参照)。初期色は `Added<StrategyFragment>` フィルタで `compute_syntax_spans_system` を 1 度走らせて `SyntaxSpans` を埋め、次フレームで `apply_highlight_layers_system` が attrs を適用
  - editor entity spawn 時に `SyntaxSpans::default() / FindMatchSpans::default() / BracketSpans::default()` を一緒に挿入
  - `set_wrap(Wrap::None)` を 1 回呼ぶ
  - `EDITOR_LINE_HEIGHT` を `EDITOR_METRICS` 定数 (`Metrics::new(14.0, 18.0)`) に格上げして全モジュールで共有 (`const` 不可なら `pub fn editor_metrics() -> Metrics`)
- `src/ui/components.rs` — composer が乗せる固定色のみ追加: `BRACKET_MATCH_FG`, `FIND_MATCH_FG`, `FIND_CURRENT_MATCH_FG` (いずれも foreground)。**SYNTAX_* 定数は追加しない** (syntect の Theme が foreground を持っているので不要)。**`FIND_MATCH_BG` / `FIND_CURRENT_MATCH_BG` は Phase A v1 では追加しない** — cosmic_text::Attrs に背景色 API が無いため AttrsList では塗れない。背景色ハイライトが必要なら Phase E v2 で別 Sprite overlay layer (match range の rect を editor Transform に重ねる) として設計する

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
- `bevy_cosmic_edit::CosmicEditBuffer::with_text` (`crates/bevy_cosmic_edit/src/buffer.rs`) — `spawn_strategy_editor_panel` の seed 注入はそのまま維持。**初期色付けは `Added<StrategyFragment>` で `compute_syntax_spans_system` を 1 度走らせ、次フレームで `apply_highlight_layers_system` が attrs を組み立てる**。以降の更新も attrs だけ差し替え (composer 経由) + `set_redraw(true)` 明示
- `bevy_egui` — **本プランでは使わない** (Cargo.toml にも未登録)。Find パネル含め全 UI を Sprite + Text2d + bevy_cosmic_edit の世界空間ウィンドウで揃える
- `Res<ButtonInput<KeyCode>>` + `FocusedWidget` 判定 — Tab/Enter/Ctrl+F の検出
- `EventReader<KeyboardInput>` (read-only) — bracket autoclose の文字判定。`Events::clear()` は **呼ばない** (cosmic_edit が opener を入れるのを邪魔しない、Caveat 5 参照)。`menu_bar.rs` Alt+F/E は cosmic_edit を完全に黙らせる用途で `clear()` を使っているが本タスクの用途とは違うので混同しない

## Caveat 一覧 (本タスクで踏みうるもの)

1. **`set_rich_text` はカーソルを (0,0) にリセット** — Phase A は初期化のみで使う。以降は `BufferLine::set_attrs_list` で attrs だけ更新する (cosmic_text 0.12 で API 確認済: `BufferLine::set_attrs_list(AttrsList) -> bool`)
2. **focused / unfocused で描画されるバッファが違う** — render.rs:88 のコメント通り、focused なら editor 内部 buffer、unfocused なら CosmicEditBuffer が描画される。`for_each_buffer` ヘルパは editor 側があれば editor のみ、無ければ CosmicEditBuffer のみを更新する (両更新は不要、focus 切替時に editor 側へ書き戻される設計)
3. **cosmic_edit Enter は `ButtonInput<KeyCode>::just_pressed` で読まれている** — `Events<KeyboardInput>.clear()` では止まらない。`ResMut<ButtonInput<KeyCode>>::reset(KeyCode::Enter)` を `.before(bevy_cosmic_edit::input::InputSet)` で呼ぶ
4. **Tab は cosmic_edit が黙って吸う** — `Key::Tab` は `Key::Character` ではないので `match _ => ()` で無視される。Phase C で `Action::Insert(' ') × 4` を発火させても二重発火しないが、将来防衛として `.before(InputSet)` は付ける
5. **bracket autoclose の順序は逆** — opener (`(`) は cosmic_edit に挿入させ、closer (`)`) を我々が後置する。`.after(bevy_cosmic_edit::input::InputSet)` で動かし、`Events<KeyboardInput>.clear()` は **絶対に呼ばない** (呼ぶと opener も入らなくなる)
6. **syntect 初期化コスト** — `SyntaxSet::load_defaults_newlines()` + `ThemeSet::load_defaults()` で数十〜百 ms。**Startup system で 1 回だけ実行**し `Resource` に保持。Update で呼ばない。Resource トレイトは `Send + Sync` 必須だが syntect の `SyntaxSet` / `Theme` は両方満たすので素の `Resource` で OK (tree-sitter `Parser` の `!Sync` 問題は Phase F まで発生しない)
7. **`set_wrap` は `&mut FontSystem` 必須** — `editor_buffer.0.set_wrap(&mut font_system.0, Wrap::None)` の形で呼ぶ。`CosmicEditBuffer` 直叩きでも `&mut FontSystem` が要る。**Phase B の widget サイズ変更後に `Wrap::None` が維持されているか目視確認**、上書きされていれば毎フレーム再設定する system を追加
8. **AttrsList 重ね順は composer が一元管理** — syntax / find / current_find / bracket の重ね順は `compose_attrs_for_line` 内で固定 (`default → syntax → find → current_find → bracket`)。**各 compute_*_spans system は `set_attrs_list` を絶対に呼ばない**、Span Component の書き換えに徹する。`apply_highlight_layers_system` だけが `set_attrs_list` を呼ぶ。これで Phase E / bracket クリア時の「前回範囲を syntax 色で部分復元」のような状態跨ぎ復元ロジックが不要になり、dirty 行を毎回再生成すれば正しい色に収束する
9. **`Scroll::line` は layout 行、`Scroll::vertical` は line 内 pixel** — wrap を None に固定すれば source 行 == layout 行で gutter と一致
10. **Undo/Redo 後の再ハイライト** — `fragment.source` を書き換える経路 (PendingStrategySnapshotRestore) を通れば自動で `Changed<StrategyFragment>` が立つ。dirty フィールドには触らない
11. **`fragment.dirty` は autosave 専用** — highlight 用には Bevy 標準の `Changed<StrategyFragment>` を使う (二重利用は競合の元)
12. **Find target_editor の lifecycle** — 開いた瞬間の `FocusedWidget.0` を `target_editor` に保存、Esc 時にそこへ戻す。target editor が despawn 済みなら state をリセット (`q.get(e).is_err()` チェック)
13. **`EditorScrollThumb` は `target_editor: Entity` を carry する** — multi-spawn で thumb が複数並ぶため、observer から「どのエディタを操作するか」を引けるようにする
14. **EDITOR_PANEL_SIZE と EDITOR_TEXT_SIZE を分離** — 既存 `EDITOR_SIZE` は panel サイズ意味で残し、エディタ Sprite には `EDITOR_TEXT_SIZE = panel - gutter - scrollbar` を渡す。混同するとパネルごと縮む
15. **`add_systems` タプル 20 上限** — Phase A〜E で 10 個以上のシステムを追加するので、既存登録数次第で chain 分割
16. **tree-sitter は Phase A v1 では扱わない** — ABI 整合 / `highlights.scm` の取得・ライセンス・出典コメント (`;` 開始) / byte range と cosmic_text の line/index 変換 / Query capture → 色マッピングはどれも v1 で消化するには重い。Phase A v1 は **syntect 一択**。tree-sitter は Phase F (incremental が本当に必要になったら) で扱う
17. **syntect Python サポートは default set に含まれる** — `SyntaxSet::load_defaults_newlines()` には Python が含まれる (`find_syntax_by_extension("py")` で取得可能)。追加 .sublime-syntax アセットは不要。Theme も `ThemeSet::load_defaults()` の `base16-mocha.dark` で実用十分
18. **bevy_egui は Cargo.toml に無い** — Find パネルは egui を使わず、既存 floating window パターン (Sprite + CosmicEditBuffer × 2) で組む
19. **`StrategyFragment` と `CosmicEditBuffer` は別 entity** — root entity に `WindowRoot + StrategyFragment + StrategyEditorId`、child editor entity に `StrategyEditorContent + CosmicEditBuffer + Option<CosmicEditor> + StrategyEditorId + SyntaxSpans + FindMatchSpans + BracketSpans`。1 つの Query で両方は取れない。**`fragments_q: Query<.., (With<WindowRoot>, Changed<StrategyFragment>)>` + `editor_q: Query<.., With<StrategyEditorContent>>` の 2 段ジョイン**を `StrategyEditorId.region_key` で行う (既存 sync 群と同じパターン)
20. **`BufferLine::set_attrs_list` 単独では再描画されない** — `set_attrs_list` 内部の `reset_shaping` は字形再計算フラグであって、render が見る Buffer 全体の `redraw` flag は別。attrs 更新後に **`buffer.set_redraw(true)`** (CosmicEditBuffer なら `b.set_redraw(true)`、editor 内部 buffer なら `editor.with_buffer_mut(|b| b.set_redraw(true))` + `editor.set_redraw(true)`) を明示
21. **Find editor は `StrategyEditorContent` を絶対に付けない** — 専用マーカー `FindQueryEditor` / `FindReplacementEditor` を使う。誤って `StrategyEditorContent` を付けると Find 入力の `CosmicTextChanged` が `sync_editor_to_strategy_buffer_system` に拾われ、Strategy Editor の `fragment.source` が Find 文字列で上書きされる事故になる。さらに composer も `With<StrategyEditorContent>` フィルタなので Find editor は plain text 表示 (それで正しい)
22. **bracket span は別行をまたぐ + 部分復元しない** — opener と closer は通常別行にある (`pair: [(line, range); 2]`)。**「前回の bracket 範囲だけ syntax 色に戻す」ロジックは書かない**。代わりに `BracketSpans` に `pair` と `prev_pair` を持たせ、両方の行を dirty に入れて composer に再生成させる (= 各行は SyntaxSpans + FindMatchSpans + 現在の BracketSpans から毎回フル合成される)。これで bracket クリア時の syntax 色復元事故が構造的に消える
23. **incremental parse / tree-sitter は Phase F に降格** — Phase A v1 (syntect 全文再トークナイズ) は Python 1KB で数 ms、500 行でも `Changed<StrategyFragment>` 駆動なのでアイドル時 0 コスト。**実測で目視ラグが観測されない限り Phase F は着手しない**。先に着手すると `Tree::edit(&InputEdit)` の diff 計算経路 (`CosmicTextChanged` は全文しか持たない) の設計拡張が必要になり、composer 以降の安定化を後回しにすることになる
24. **Phase C のカスタム編集は `CosmicTextChanged` を手動 send** (Critical) — `crates/bevy_cosmic_edit/src/input.rs:518` で確認した通り `CosmicTextChanged` は cosmic_edit input system 内で `is_edit = true` のときだけ発火する。Tab (cosmic_edit が無視)、Enter (我々が `keys.reset` で抑止)、bracket closer (我々が `editor.action(Action::Insert(closer))` を直接呼ぶ) のいずれも cosmic_edit のイベント発火パスを通らない。**各カスタム編集 system の末尾で `EventWriter<CosmicTextChanged>` を経由して `(entity, editor.with_buffer_mut(|b| b.get_text()))` を手動 send** しないと、`sync_editor_to_strategy_buffer_system` → `fragment.source` 更新 → autosave / undo / 再 highlight すべてが空振りする。同一フレーム内に同じ entity / 全文を 2 度 send しても既存 sync 系の short-circuit (`fragment.source == *new_text` で continue) で安全
25. **初期 seed を `with_rich_text([("", ...)], ...)` で空 spans に置換しない** (Critical) — 現行 `src/ui/strategy_editor.rs:154-162` は `with_text(font_system, &seed, default_attrs)` で seed を buffer 構築と同時に注入している。空 `with_rich_text` に置換すると seed が消える (Phase A v1 の highlight は `set_attrs_list` で attrs だけ差し替えるため、テキストは挿入されない)。**`with_text` を維持し、初期色は `Added<StrategyFragment>` フィルタで 1 度だけ highlight を走らせる** か、`with_rich_text(&[(seed.as_str(), default_attrs)], default_attrs)` で seed を span として渡す
26. **既存 4-system チェーンを並べ替え・削除しない** (High) — 現行 `src/ui/mod.rs:230-237` は `sync_editor_to_strategy_buffer → undo_redo → apply_pending_app_edits → apply_strategy_snapshot_restore → sync_strategy_buffer_to_editor` の順で固定済 (Undo/Redo を `PendingStrategySnapshotRestore` → `UndoRedoApplied` 経由で動かす設計)。新規 highlight 系 (6-9) はこの末尾に `.after(sync_strategy_buffer_to_editor_system)` で**後置のみ**。並べ替えると undo 直後の再 highlight が 1 フレーム遅延する
27. **Find パネルは `Option<Entity>` で lifecycle 管理** (High) — `is_open == true` のとき毎フレーム spawn する書き方では Bevy 0.15 で panel entity が毎フレーム積み重なる (Zed の `BufferSearchBar` は GPUI View ライフサイクルで自動消滅するが我々の Sprite ベース実装には無い)。`FindReplaceState::panel_root: Option<Entity>` で spawn 済みかを判定し、false→true 遷移で 1 回 spawn、true→false 遷移で `despawn_recursive`。query_editor / replacement_editor の child entity も Resource に保存
28. **Find editor の入力は `sync_find_editors_to_state_system` で `FindReplaceState.query/replacement` に書き戻す** (High) — `FindQueryEditor` / `FindReplacementEditor` マーカーを付けた editor は既存 `sync_editor_to_strategy_buffer_system` が無視するが、誰も `state.query` を更新しなければ `find_match_recompute_system` の `Changed<FindReplaceState>` 駆動が永久に発火しない。専用 sync system を `(With<FindQueryEditor>, Without<StrategyEditorContent>)` フィルタで追加。history / autosave には絶対に触らない
29. **Bevy 0.15 の observer trigger は `trigger.entity()`** — `trigger.target()` は Bevy 0.16+ で導入された rename API。本リポジトリは Bevy 0.15 (`src/ui/floating_window.rs:55-63` で `trigger.entity()` を使用)。Phase B の `EditorScrollThumb` Pointer<Drag> observer もここに揃える。コピペで `trigger.target()` を書くと compile error

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
   - `def`, `class`, `import`, 文字列, コメントが色分け (syntect の base16-mocha.dark)
   - カーソル前の `(` に対応する `)` がハイライトされる
   - **opener と closer が別行にあっても両方ハイライトされ、カーソルが動いたら両方クリアされる** (= composer の dirty 行再生成だけで syntax 色が戻る、prev_pair 範囲を部分復元する処理が不要)
   - **長文をタイプしてもカーソルが先頭に飛ばない**
   - **Undo/Redo (Ctrl+Z/Y) 直後にも色が抜けない、白いフラッシュが 1 フレームも出ない** (システム順序の検証)
   - **Find ハイライト中に bracket pair をいくつか跨いでカーソル移動しても、Find マッチ色が消えない** (composer 固定順序の検証 — syntax → find → current_find → bracket)
4. **Phase B**: 
   - 左に行番号、右に scrollbar thumb が表示されスクロールに追従
   - thumb をドラッグして移動できる
   - 行番号がエディタの行高にズレなく揃う
   - **長い行 (200 文字以上の 1 行) を入力したときに折り返されず横にはみ出すこと** (`Wrap::None` が widget 経由で上書きされていないことの確認)
5. **Phase C**: Tab で 4 spaces 入る、Enter 後に前行のインデントが継承される、`(` で `)` が自動補完されカーソルが間に残る、`)` の前で `(` を打っても `))` にならない
   - **Tab / Enter / bracket autoclose の直後に Ctrl+Z で 1 stroke 単位で undo できる** (`CosmicTextChanged` を手動 send している確認 — send 漏れだと undo 履歴に積まれない)
   - **Tab / Enter / bracket autoclose 後に 1 秒待つと cache へ autosave される** (autosave 経路の確認 — send 漏れだと dirty が立たず保存されない)
   - **Enter 直後にカスタム highlight 系も追従** (改行で行数が増えたことが gutter / scrollbar / syntax 全部に伝わる)
6. **Phase E**: 
   - Ctrl+F で Find パネル開く、`def` を検索して全マッチ強調 + 最初のマッチへスクロール
   - **Ctrl+F を 5 回連打しても Find パネルは 1 つしか表示されない** (`panel_root: Option<Entity>` lifecycle の確認 — ガード漏れだと 5 つ重なる)
   - **Find パネルを開いて Esc で閉じ、また Ctrl+F で開ける** (despawn → 再 spawn 経路の確認)
   - Enter で次へ、Esc で閉じてエディタにフォーカス戻る
   - find 中に対象エディタを × で閉じてもクラッシュしない (`panel_root` の存在検証で state リセット)
   - **Find パネルの query 欄に文字を打つと、入力した文字列で対象 Strategy Editor のマッチが即時更新される** (`sync_find_editors_to_state_system` の確認 — sync 漏れだと query 欄に打っても何も起きない)
   - **Find パネルの query 欄に文字を打っても、対象 Strategy Editor の本文が書き換わらない** (マーカー分離の確認)
7. Undo (Ctrl+Z) 後に色付け・行番号・スクロールバーすべて正しく再描画される
8. Multi-spawn (region_002 等) でも各エディタに独立して上記が動く

### 既存機能の非退行
- Auto-save (1 秒 debounce) が引き続き動く (`fragment.dirty` 経路に手を入れていないこと)
- ドラッグでウィンドウ移動 + Undo
- Ctrl+S/O での Save/Load (layout JSON 経由)
- Sidecar JSON (`<strategy>.json`) との連携

## 実装方針メモ

- **pair-relay 移行候補**: Phase A だけで attrs/composer 基盤の地ならしで 300〜500 行、全フェーズ完遂は 1 セッションでは厳しい。Phase A 着手前に `pair-relay` スキルへ移行、本プランを Navigator に引き継ぐのが安全。Navigator は事前に `bevy-engine` スキルで Bevy 0.15 罠 (observer の import path、Anchor 左寄せ) を、`zed` スキルで HighlightKey 別レイヤー → 描画時合成の先行事例 (`crates/editor/src/display_map/custom_highlights.rs`) を必ず読む
- **Bevy 0.15 罠**: `add_systems` タプル 20 上限、observer の import path (`bevy::ecs::observer::Trigger`)、Anchor 左寄せ (`bevy::sprite::Anchor::CenterLeft`) は `bevy-engine` スキル発動で都度確認
- **syntect API 確認手順**: 着手 1 コミット目で `examples/syntect_smoke.rs` を作り、以下を `cargo run --example` で先に確認する。API が想定と違ったら設計に戻る (が、syntect は 5.x で安定しており大きく変わる可能性は低い)。
  1. `SyntaxSet::load_defaults_newlines()` が `find_syntax_by_extension("py")` を返すこと
  2. `ThemeSet::load_defaults()` のキー一覧 (`base16-mocha.dark` の存在確認)
  3. `HighlightLines::highlight_line(line, &syntax_set)` の戻り値が `Vec<(Style, &str)>` であること
  4. `Style.foreground` が `(r, g, b, a)` 形式で `cosmic_text::Color::rgba` にそのまま渡せること
  5. `default-fancy` feature で Windows ビルドが onig 無しで通ること
- **composer の TDD**: `compose_attrs_for_line` は純粋関数なので、`#[cfg(test)] mod tests` で以下を担保:
  1. 全空 spans → base 1 つの AttrsList
  2. syntax 1 span のみ → 該当 range だけ色変化
  3. syntax + find 重なり → find が勝つ (順序)
  4. syntax + find + bracket すべて重なり → bracket が勝つ
  5. find と current_find の同じ位置 → current_find が勝つ
- **Phase F (将来) への布石**: `compute_syntax_spans_system` の interface (`SyntaxSpans` を吐く) を Phase A v1 で固めておけば、Phase F で tree-sitter に差し替えるときに composer 以降は無変更。Phase F 着手時の確認事項 (Phase A v1 では扱わない):
  1. tree-sitter ABI 整合 (`Parser::set_language` の引数型、`tree_sitter_python::LANGUAGE` vs `language()`)
  2. `highlights.scm` の取得経路 (Zed grammar コピー or upstream crate `include`)、ライセンス、`;` コメント
  3. byte range と cosmic_text line/index の変換
  4. Query capture index → 色のマッピング
  5. `Tree::edit(&InputEdit)` の incremental 経路 (`CosmicTextChanged` 全文 → diff 計算 → `StrategyFragmentEdited { entity, input_edit }` Event 配信)
  6. `Parser` が `!Sync` なので `NonSend` 登録、`Mutex<Parser>` は無駄

