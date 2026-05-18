---
name: zed
description: Phase 7.2「Monaco-Grade Strategy Editor」(docs/plan/Phase 7.2 - Monaco-Grade Strategy Editor.md) を実装する際の必読スキル。Bevy + bevy_cosmic_edit + syntect で組む Strategy Editor (src/ui/strategy_editor.rs, src/ui/components.rs, src/highlight.rs) に Monaco / VSCode 級の機能 — シンタックスハイライト, 行番号ガター, ステータスバー, スクロールバー, オートインデント, Tab→4sp, Find & Replace, ブラケットマッチ, 診断 — を載せる作業全般で発動する。Zed エディタのソースコードミラー (.claude/skills/zed/src/crates/) を「実装の正解」として参照し、GPUI 固有 API は Bevy ECS / cosmic_text / syntect に翻訳して提案する。トリガー語: "Phase 7.2", "Strategy Editor", "strategy_editor.rs", "シンタックスハイライト", "syntax highlight", "syntect", "Highlighter", "行番号", "gutter", "ガター", "LineNumberGutter", "status bar", "ステータスバー", "scrollbar", "scroll thumb", "EditorScrollThumb", "auto indent", "オートインデント", "tab_to_spaces", "Find & Replace", "FindReplaceState", "ブラケットマッチ", "bracket match", "diagnostics", "SyntaxDirty", "set_rich_text", "highlight_python", "Monaco", "VSCode", "code editor", "コードエディタ", "Python エディタ" 等が出たら必ず起動する。src/ui/strategy_editor.rs を Edit/Write しようとする前にも必ず読むこと（cosmic_edit と Zed のメンタルモデル差分でハマる)。
---

# zed — Phase 7.2 実装ガイド

## 何のためのスキルか

The-Trader-Was-Replaced の Strategy Editor (Bevy + bevy_cosmic_edit) を **Monaco / VSCode 級** に育てる Phase 7.2 を実装するとき、毎回ゼロから設計するのではなく、**Zed エディタの実装を「先行事例」として参照しながら**、それを Bevy ECS / cosmic_text / syntect に翻訳して提案するためのスキル。

Zed は Rust 製の本物の production-grade エディタで、Phase 7.2 で必要な機能はすべて Zed の中に「動いている実装」として存在する。我々の方が制約は強い (cosmic_edit は単一 Buffer ベース・rope は無い・LSP も無い) が、**「どういう責務分割で書かれているか」「どこにエッジケースが潜むか」は Zed を読むのが最速**。

## いつ発動するか / いつ発動しないか

**発動する:**
- `docs/plan/Phase 7.2 - Monaco-Grade Strategy Editor.md` 配下のサブステップ (7.2.0〜7.2.9) を実装するとき
- `src/ui/strategy_editor.rs` / `src/ui/components.rs` / `src/highlight.rs` / `src/ui/find_replace.rs` を Edit/Write するとき
- bevy_cosmic_edit の `Buffer::set_rich_text`, `CosmicEditor::with_buffer_mut`, `Attrs`, `AttrsOwned` を扱うとき
- syntect の `SyntaxSet`, `ThemeSet`, `HighlightLines`, `LinesWithEndings` を扱うとき
- 行番号ガター / スクロールバー / ステータスバー / Find&Replace / オートインデント / Tab/Enter キーフックを書くとき

**発動しない:**
- Strategy Editor 以外の floating window (positions, orders, chart など) → `bevy-engine` スキルへ
- Phase 7.1 以前の Undo/Redo そのものの実装 → 既に完了している前提で配線だけ扱う
- バックエンド (Python/gRPC) 側の構文チェック RPC 設計 → 本スキル範囲外 (将来 P3)

## 前提知識 (まず把握すること)

### 我々のスタック
- **Bevy 0.15** ECS — system / Resource / Component / Query / EventReader
- **bevy_cosmic_edit (ローカルフォーク)** `crates/bevy_cosmic_edit/` — `CosmicEditBuffer`, `CosmicEditor`, `TextEdit2d`, `FocusedWidget`, `CosmicBackgroundColor`
- **cosmic_text** — `Buffer`, `Attrs`, `AttrsOwned`, `Color`, `Shaping`, `Metrics`
- **syntect 5** (これから追加) — fancy-regex バックエンド, Windows 対応

`bevy-engine` スキル + `MEMORY.md` の **cosmic-edit Buffer メトリクスの DPI トラップ**, **cosmic-edit unfocused field 文字非表示** を必ず先に読むこと。Phase 7.2 でも同じ罠を踏む。

### Zed のスタック (我々には**ない**もの)
- **GPUI** — Zed 独自の retained-mode UI フレームワーク (Bevy ECS とは別物)
- **Rope** (`crates/rope`, `crates/text`) — 大規模テキスト用の永続データ構造
- **MultiBuffer** (`crates/multi_buffer`) — 複数 Buffer の仮想結合
- **tree-sitter** ベースのインクリメンタルパーサ (`crates/language/src/syntax_map`)
- **LSP** クライアント全部

**翻訳ルール:**
- GPUI の `Element` / `View` → Bevy の **Component + spawn ツリー**
- GPUI の `Subscription` / `Observable` → Bevy の **Event + EventReader + `Changed<T>`**
- Rope → cosmic_text の `Buffer` (＋我々の `StrategyBuffer.source: String`)
- tree-sitter インクリメンタル → syntect の**全文再トークナイズ** (毎フレームではなく `SyntaxDirty` フラグ駆動)
- LSP → **無し** (P3 で gRPC `CheckSyntax` または `rustpython-parser` を検討)

これらの翻訳を**勝手にやらない**。Zed の責務分割 (どの関数が何を計算しているか) は真似て、API は cosmic_text / Bevy に置き換える。

## サブステップ → Zed ソース対応表

サブステップを実装する前に、対応する Zed のファイルを Read で開いて**「Zed はこの責務をどこに置いているか」「edge case として何を扱っているか」を 5 分眺める**こと。コピペするのではなく、**設計の感覚を借りる**ために読む。

`Z:` プレフィックスは `.claude/skills/zed/src/` を指す。

| Sub-step | 機能 | Zed 参考実装 | 我々の翻訳先 |
|----------|------|--------------|--------------|
| 7.2.0 | Undo/Redo 配線に `SyntaxDirty` を仕込む | `Z:crates/multi_buffer/src/transaction.rs`, `Z:crates/editor/src/editor.rs` の `transact` / `end_transaction` 周辺 | `sync_strategy_buffer_to_editor_system` と `sync_editor_to_strategy_buffer_system` の両方で `syntax_dirty.0 = true` |
| 7.2.1 | `syntect` を Cargo.toml に追加 | (Zed は tree-sitter を使うので直接の参考なし) | `syntect = { version = "5", default-features = false, features = ["default-syntaxes", "default-themes", "fancy-regex"] }` — Windows で onig (C) を避けるため fancy-regex 必須 |
| 7.2.2 | `src/highlight.rs` — `Highlighter` Resource | `Z:crates/language/src/syntax_map.rs` (パーサ保持の仕方), `Z:crates/syntax_theme/src/syntax_theme.rs` (テーマ→色), `Z:crates/editor/src/display_map/custom_highlights.rs` (色を画面に乗せる側) | `SyntaxSet::load_defaults_newlines()` + `ThemeSet::load_defaults()` を Resource に保持。`highlight_python(&str) -> Vec<(&str, AttrsOwned)>` を提供。Startup で 1 回だけ生成。 |
| 7.2.3 | `apply_syntax_highlight_system` | `Z:crates/editor/src/display_map.rs`, `Z:crates/editor/src/display_map/custom_highlights.rs` (どのタイミングで再計算するか), `Z:crates/editor/src/highlight_matching_bracket.rs` (差分的 highlight 更新の感覚) | `SyntaxDirty` フラグ駆動で `CosmicEditBuffer` と `CosmicEditor` 両方に `set_rich_text`。**MEMORY #4545** の罠を必ず踏まないこと |
| 7.2.4 | 行番号ガター | `Z:crates/editor/src/element.rs` の `paint_gutter` / `layout_gutter` 周辺, `Z:crates/editor/src/indent_guides.rs` | `LineNumberGutter` Component を持った `Text2d` entity を content_area 左に並べる。`buffer.is_changed()` で更新。フォントメトリクスは `EDITOR_LINE_HEIGHT` と完全一致させる |
| 7.2.5 | ステータスバー | `Z:crates/go_to_line/src/go_to_line.rs` (行/列の取り方), Zed 側は workspace の status bar crate (`crates/status_bar` 等) を使うが、我々は単一 Text2d で十分 | `CosmicEditor::cursor()` で `(line, index)` 取得。`history.record.can_undo()` で `↩` インジケータ。`StatusBar` Component |
| 7.2.6 | Tab → 4 スペース | `Z:crates/editor/src/editor.rs` の `tab` / `tab_prev` action, `Z:crates/language/src/language_settings.rs` の `tab_size` | `KeyboardInput` を `EventReader` で読み `Key::Tab` を捕まえる。**cosmic_edit fork の `crates/bevy_cosmic_edit/src/input.rs` で Tab の default を無効化必須** (二重挿入バグ) |
| 7.2.7 | オートインデント | `Z:crates/editor/src/editor.rs` の `newline` / `newline_above` / `newline_below` action, `Z:crates/language/src/language.rs` の `indent_size_for_line` / `IndentSize`, `Z:crates/editor/src/jsx_tag_auto_close.rs` (auto-* 系 system の構造例) | `KeyboardInput` で `Key::Enter` を捕まえ、`CosmicEditor::cursor()` で前行を取り `len - trim_start().len()` でインデント幅。前行が `:` で終わるなら +4。`insert_string(&indent, None)` |
| 7.2.8 | スクロールバー | `Z:crates/editor/src/scroll.rs`, `Z:crates/editor/src/scroll/autoscroll.rs`, `Z:crates/editor/src/scroll/scroll_amount.rs` (スクロール量の概念), `Z:crates/editor/src/element.rs` の scrollbar paint | `EditorScrollThumb` Component を `Sprite` entity に。`CosmicEditor::with_buffer(|b| b.scroll().vertical)` で位置取得。トラック高/サム高は計画書の式そのまま |
| 7.2.9 | Find & Replace | `Z:crates/search/src/buffer_search.rs`, `Z:crates/search/src/search_bar.rs`, `Z:crates/search/src/search.rs` (state machine 全般) | `FindReplaceState` Resource + `bevy_egui::egui::Window` で UI。マッチハイライトは `apply_syntax_highlight_system` の**後**に黄色 `Attrs` で上書き (順序重要) |
| 将来 P2 | ブラケットマッチ | `Z:crates/editor/src/highlight_matching_bracket.rs`, `Z:crates/editor/src/bracket_colorization.rs` | カーソル位置で前後 1 文字見て `([{` ↔ `)]}` をペアスキャン。マッチ位置に `Attrs::color()` で上書き |
| 将来 P3 | エラー診断 | `Z:crates/diagnostics/src/diagnostics.rs`, `Z:crates/diagnostics/src/diagnostic_renderer.rs`, `Z:crates/diagnostics/src/buffer_diagnostics.rs`, `Z:crates/language/src/diagnostic.rs` | ガター entity に `⚠` を行単位で重ねる。Python AST パースは `rustpython-parser` か gRPC RPC |

## Zed を読むときの読み方

時間を浪費しないために:

1. **対応表のファイルだけ開く**。Zed は数百 crate あるが、Phase 7.2 で見るべきは上の表だけ。
2. **`pub fn` のシグネチャだけ眺める** → 責務分割を掴む。中身の GPUI / Rope 呼び出しは無視。
3. **コメント / doc comment を読む** → Zed の rationale が分かる (例: なぜ rope なのか、なぜ MultiBuffer か)。
4. **テストファイル** (`*_tests.rs`) があれば**先にそちらを読む** — エッジケースが入力例として並んでいる。例: `Z:crates/editor/src/editor_tests.rs` の auto-indent / bracket match テスト。
5. **GPUI 固有 API (`cx.notify()`, `View`, `Element`) は読み飛ばしてよい** — 我々は Bevy ECS なので翻訳が要る。代わりに「どの state を変更したか」「どのイベントを発火したか」だけ抽出する。

## 翻訳パターン早見表

| Zed (GPUI) | 我々 (Bevy + cosmic_edit) |
|------------|--------------------------|
| `cx.subscribe(&buffer, |this, _, event, cx| ...)` | `EventReader<CosmicTextChanged>` |
| `cx.notify()` (再描画要求) | `buffer.set_redraw(true)` ＋ `Changed<T>` |
| `buffer.read(cx).text()` | `StrategyBuffer.source` (String) |
| `buffer.update(cx, |b, cx| b.edit(...))` | `editor.insert_string(s, None)` ＋ `Record::edit` push |
| `HighlightMap` / `SyntaxLayer` | `syntect::easy::HighlightLines` |
| `editor.scroll_position(cx)` | `editor.with_buffer(|b| b.scroll().vertical)` |
| `editor.selections.newest::<usize>(cx)` | `editor.cursor()` (単一カーソル前提) |
| `Workspace::register_action(editor::Tab, ...)` | `KeyboardInput` EventReader + `Key::Tab` 判定 |
| Zed の `KeymapContext` (`vim_mode && editing`) | `FocusedWidget` Resource で focused entity を見る |
| GPUI の `div().child(line_numbers)` | 別 entity を spawn して content_area の子にする |

## 必ず守る Caveat (memory と過去 PR から)

1. **CosmicEditor 内部 buffer 問題** (MEMORY: cosmic-edit-buffer-metrics-dpi-trap.md と関連 #4545)
   `set_rich_text` は `CosmicEditBuffer` と `CosmicEditor::with_buffer_mut` の**両方**に呼ぶ。focused フィールドは editor 内部 buffer を見るので、片方だけだと色が反映されない。

2. **Sprite tint 問題** (MEMORY #4527)
   ガター / スクロールバーの背景は `CosmicBackgroundColor` か Sprite だが、`Sprite.color` を暗色にすると tint が乗って想定外。`Sprite.color = Color::WHITE` ＋ image / 別 Component で色管理。

3. **Tab / Enter キーの二重処理**
   cosmic_edit の input system が同じキーを処理して二重挿入になる。`crates/bevy_cosmic_edit/src/input.rs` で Tab / Enter の default を無効化するパッチを当てた上で、我々の system だけが処理する状態にする。

4. **syntect の初期化コスト**
   `SyntaxSet::load_defaults_newlines()` は数十〜数百ms。**`Startup` で 1 回**だけ実行して `Highlighter` Resource に持つ。`Update` で呼ぶと毎フレーム重い。

5. **`set_rich_text` の存在確認**
   bevy_cosmic_edit 0.26 / cosmic_text の `Buffer::set_rich_text` API シグネチャを `crates/bevy_cosmic_edit/` で先に Grep で確認。存在しない / 引数が違う場合は `set_line_attributes` / `BufferLine::set_attrs_list` でフォールバック。

6. **Undo/Redo 後のハイライト再適用**
   `history.replaying = true` の間は `sync_editor_to_strategy_buffer_system` の `Record::edit` push をスキップするが、**`apply_syntax_highlight_system` はスキップしない**。Undo/Redo 後こそ色を貼り直す必要がある。`sync_strategy_buffer_to_editor_system` 側で `syntax_dirty.0 = true` を立てる流れを system ordering で保証。

7. **行番号ガターのフォントメトリクス**
   `Text2d` のフォントメトリクス (line_height) を editor 本体と **完全一致**させないと行がズレる。`EDITOR_LINE_HEIGHT` を共通定数化して両方で使う。MEMORY: cosmic-edit-unfocused-field-invisible-text.md の DPI / line_height の罠と同じカテゴリの問題。

8. **Find マッチハイライトの順序**
   `apply_syntax_highlight_system` で構文色を塗った**後**に、Find マッチを別 system で黄色 `Attrs` で上書きする。逆順だと Find のハイライトが構文色に上書きされて消える。system ordering で `find_highlight_system.after(apply_syntax_highlight_system)`。

## 推奨ワークフロー (1 サブステップ = 1 turn 目安)

サブステップを始めるたびに以下を実行:

1. 計画書 `docs/plan/Phase 7.2 - Monaco-Grade Strategy Editor.md` の該当サブステップ節を Read で開く
2. 上の対応表で対応する Zed ファイルを 1〜3 個 Read で開いて 5 分眺める (`pub fn` とコメントだけ)
3. 我々のコード (`src/ui/strategy_editor.rs` 等) を Read して既存構造を確認
4. **設計を 3-5 行で要約してからコードを書く** (どの Component / Resource / system / event を足すか)
5. Caveat 一覧と照合 (特に 1, 3, 4, 6 は毎サブステップで踏みうる)
6. 実装 → `cargo check` → `cargo test --lib` → 目視 E2E

複数サブステップにまたがる長い実装になる場合は `pair-relay` スキルへ切り替え、本スキルの内容を Navigator に引き継ぐこと。`src/ui/**` を触るので `bevy-engine` スキルも併用。

## 完了判定

計画書末尾の「完了条件」チェックリストを満たすこと。特に:

- Python ファイルを開いて keyword / string / comment が色分けされる
- **Ctrl+Z で Undo した後もシンタックスハイライトが正しく再適用される** (Caveat 6 のテスト)
- Tab / Enter が二重挿入されない (Caveat 3)
- `cargo check` / `cargo test` が通る
- E2E: `test_strategy_daily.py` を開いて色付き → 編集 → Ctrl+Z → 色が再適用される

完了したら `post-impl-skill-update` を起動して本スキルの description / 対応表 / Caveat を実情にアップデートすること。Zed の該当ファイルパスは Zed 側 upstream の整理でズレる可能性があるので、その時点での実在を Glob で再確認してから更新する。
