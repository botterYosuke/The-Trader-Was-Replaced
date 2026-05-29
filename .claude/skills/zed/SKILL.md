---
name: zed
description: |
  Use this skill BEFORE editing `src/ui/**` in The-Trader-Was-Replaced, or whenever the user wants editor/IDE-style UI in our Bevy + bevy_cosmic_edit + bevy_egui + syntect frontend. Ships a Zed editor source mirror at `.claude/skills/zed/src/crates/` so you can read how a production desktop editor solves the same problem before writing Bevy code.

  Trigger eagerly on:
  - Any `src/ui/` file (`strategy_editor`, `instrument_picker`, `sidebar`, `menu_bar`, `footer`, `floating_window`, `layout_persistence`, `components`, `chart`, etc.) or a new panel/window/sidebar section.
  - Editor vocabulary: gutter, scrollbar, syntax highlight, find/replace, auto-indent, bracket match, undo/redo, fuzzy picker, file finder, command palette, action dispatch, keybindings (Ctrl+P, Cmd+Shift+P, Alt+F), dock, status bar, breadcrumbs, toast, modal, theme/color tokens, dark/light, layout persistence, Phase 7.x.
  - Mentions of Zed, VSCode, Monaco, or "production editor".

  Skip backend, generic Bevy ECS, tests, E2E — dedicated skills exist.
---

# zed — Bevy UI を Zed 参照で設計するスキル

## 何のためのスキルか

The-Trader-Was-Replaced の Bevy フロントエンドで新しい UI 機能を作る／既存機能を磨くときに、毎回ゼロから設計するのではなく、**Zed エディタの「動いている実装」を先行事例として参照しながら**、それを Bevy ECS / cosmic_text / bevy_egui / syntect に翻訳して提案するためのスキル。

Zed は Rust 製の production-grade デスクトップアプリで、我々が src/ui/** で欲しがる機能のほとんどは Zed のどこかに既に「責務分割された形」で存在する — テキストエディタ本体、行ガター、スクロールバー、ファジー picker、コマンドパレット、project panel、dock、status bar、breadcrumbs、通知 toast、modal layer、theme、settings UI、action ディスパッチ。我々の制約 (GPUI ではなく Bevy ECS、Rope なし、LSP なし、bevy_cosmic_edit のみ) のせいでそのまま写経はできないが、「どう責務を分割するか」「どこに edge case が潜むか」は Zed を 5 分読むのが最速。

## いつ発動するか / いつ発動しないか

**発動する:**
- `src/ui/**` 配下を Edit/Write しようとしている (strategy_editor, sidebar, footer, menu_bar, instrument_picker, orders, positions, chart, layout_persistence, floating_window, run_result_panel, scenario_startup_panel, replay_startup_window 等)
- Zed が同種機能を持つ UI を新規/改修する:
  - **テキスト編集系** — syntax highlight, gutter, scrollbar, find/replace, auto-indent, bracket match, multi-cursor, diagnostics, undo/redo transaction
  - **リスト/picker 系** — fuzzy search, file finder, instrument picker, tab switcher, tickers list, virtual scroll, ハイライト付きマッチ表示
  - **コマンド/メニュー系** — command palette, menu bar, context menu, key chord, action ディスパッチ
  - **レイアウト系** — dockable panel, split pane, floating window, drag-resize, persistence, modal layer (z-order)
  - **テーマ/色** — color tokens, dark/light, syntax theme, scale
  - **検索** — in-buffer search, project-wide search, replace
  - **診断/通知** — status bar, error squiggles, notification toast, activity indicator
- bevy_cosmic_edit の Buffer/Editor API を扱う、または syntect / nucleo-matcher 等の採用判断
- 「Zed だとどう書いてる?」「VSCode/Monaco みたいに ...」「production editor 風の ...」という要望
- `Phase 7.x`「Monaco-Grade Strategy Editor」系のサブステップ実装 (旧 Phase 7.2 仕様も含む)

**発動しない:**
- バックエンド (Python/gRPC、戦略実行エンジン、scenario runner、cache) → `nautilus_trader` / `tachibana` / `kabusapi` スキルへ
- Bevy ECS / Camera2d / Sprite / bevy_egui 単体の一般論 → `bevy-engine` スキルへ
- bevy_egui で雑に作る一発もの debug HUD で Zed を参照する価値がないもの
- Rust テスト戦略 → `rust-testing` / `tdd-workflow` スキルへ
- E2E 手動検証 → `e2e-testing` スキルへ
- **`src/ui/**` を触ってもエディタ/IDE 機能の設計ではない場合** — 例: スケジュール登録 (`add_systems` / `configure_sets`)、change detection の調整、`bevy_instanced_text` / Bevy UI のキャッシュ・rebuild gate plumbing、`UiGlobalTransform` ↔ `ComputedNode` 周辺、`PostUpdate` の `LayoutProduceSet` / `TextViewRenderSet` の前後関係修正など、エディタ affordance（gutter、scrollbar、find/replace、fuzzy picker、command palette、theme）ではなく **Bevy/render pipeline の plumbing** が本体の修正は `bevy-engine` だけで十分 — Zed ソースを引いても答えが出ない（Zed は GPUI ベースで Bevy UI の change-detection 仕様を持たない）。実例: issue #50 Step 0 spike の drag/pan follow 修正は `src/ui/strategy_editor_spike.rs` + `src/ui/mod.rs` を編集したが、本体は `bevy_instanced_text` の `update_text_views` 早期 return を回避するための `DisplayLayout::set_changed()` 呼び出しであり、エディタ機能とは無関係 → zed スキル invoke は不要だった。

## 前提知識 (これは先に把握すること)

### 我々のスタック
- **Bevy 0.15** ECS — system / Resource / Component / Query / EventReader / Observer
- **bevy_cosmic_edit (ローカルフォーク)** `crates/bevy_cosmic_edit/` — `CosmicEditBuffer`, `CosmicEditor`, `TextEdit2d`, `FocusedWidget`, `CosmicBackgroundColor`
- **cosmic_text** — `Buffer`, `Attrs`, `AttrsOwned`, `Color`, `Shaping`, `Metrics`
- **bevy_egui** — modal / overlay / 簡易 UI
- **syntect 5** — fancy-regex バックエンド (Windows で onig=C を避けるため必須)、Startup で 1 回 load
- **共通既存ピース** — `src/ui/floating_window.rs`, `layout_persistence.rs`, `components.rs` (色トークン), `menu_bar.rs`, `sidebar.rs`, `button.rs`

並用前提: `bevy-engine` スキルで `add_systems` タプル 20 上限・observer・required components・Anchor の罠を先に押さえること。Phase 7.x でも同じ罠を踏む。

### Zed のスタック (我々には**ない**もの)
- **GPUI** — Zed 独自の retained-mode UI フレームワーク (Bevy ECS とは別物)
- **Rope** (`crates/rope`, `crates/text`) — 大規模テキスト用永続データ構造
- **MultiBuffer** (`crates/multi_buffer`) — 複数 Buffer の仮想結合
- **tree-sitter** ベースのインクリメンタルパーサ (`crates/language/src/syntax_map`)
- **LSP** クライアント
- **fuzzy** マッチャ (`crates/fuzzy`, `crates/fuzzy_nucleo`)

**翻訳ルールの原則:**
- GPUI `Element` / `View` → Bevy **Component + spawn ツリー**
- GPUI `Subscription` / `Observable` → Bevy **Event + EventReader + `Changed<T>`**
- `Rope` / `MultiBuffer` → cosmic_text の `Buffer` (＋我々の `*Buffer.source: String`)
- tree-sitter インクリメンタル → syntect の**全文再トークナイズ** (Dirty フラグ駆動、毎フレームではない)
- LSP → **無し** (当面は無視、診断は P3 で gRPC `CheckSyntax` か `rustpython-parser`)
- ファジーマッチ → `nucleo-matcher` を直接、または subsequence 自前で十分

これらの翻訳を**勝手にやらない**。Zed の責務分割 (どの関数が何を計算しているか) は真似て、API は cosmic_text / Bevy / egui に置き換える。

## UI ドメイン → Zed crate 対応表

新しい UI を作るときは、まず該当ドメインの Zed crate を **1〜3 個 Read** して、責務分割と edge case のリストを 5 分眺める。`Z:` プレフィックスは `.claude/skills/zed/src/` を指す。

### テキスト編集 (`src/ui/strategy_editor.rs` 系)

| 機能 | Zed 参考 | 我々の翻訳先 |
|------|----------|--------------|
| エディタ本体構造 | `Z:crates/editor/src/editor.rs` | `CosmicEditBuffer + CosmicEditor` を 1 entity、`*Buffer` Resource に `source: String` |
| 表示行マップ | `Z:crates/editor/src/display_map.rs`, `Z:crates/editor/src/display_map/custom_highlights.rs` | `Buffer::set_rich_text` を Dirty フラグ駆動で再生成 |
| syntax 適用 | `Z:crates/syntax_theme/src/syntax_theme.rs`, `Z:crates/language/src/syntax_map.rs` | syntect の `HighlightLines` を `Highlighter` Resource に、Startup で 1 回ロード |
| 行番号ガター | `Z:crates/editor/src/element.rs` (`paint_gutter` / `layout_gutter`), `Z:crates/editor/src/indent_guides.rs` | `LineNumberGutter` Component + `Text2d` を行ごとに spawn、共通 `EDITOR_LINE_HEIGHT` |
| ステータスバー (行/列) | `Z:crates/go_to_line/src/go_to_line.rs`, `Z:crates/breadcrumbs/src/breadcrumbs.rs` | `CosmicEditor::cursor()` で `(line, index)` 取得、単一 `Text2d` で十分 |
| スクロールバー | `Z:crates/editor/src/scroll.rs`, `Z:crates/editor/src/scroll/autoscroll.rs` | `EditorScrollThumb` Component + `Sprite`、`with_buffer(|b| b.scroll().vertical)` |
| Find & Replace | `Z:crates/search/src/buffer_search.rs`, `Z:crates/search/src/search_bar.rs` | `FindReplaceState` Resource + `bevy_egui::Window`、マッチは構文色の**後**に上書き |
| 自動インデント | `Z:crates/editor/src/editor.rs` (`newline` action), `Z:crates/language/src/language.rs` (`indent_size_for_line`) | `KeyboardInput` で Enter 捕獲、前行から `len - trim_start().len()` |
| Tab→spaces | `Z:crates/editor/src/editor.rs` (`tab` action), `Z:crates/language/src/language_settings.rs` (`tab_size`) | `KeyboardInput` で Tab、cosmic_edit fork の `input.rs` で default を無効化 |
| ブラケットマッチ | `Z:crates/editor/src/highlight_matching_bracket.rs`, `Z:crates/editor/src/bracket_colorization.rs` | カーソル前後 1 文字でペアスキャン、`Attrs::color()` 上書き |
| Undo/Redo transaction | `Z:crates/multi_buffer/src/transaction.rs` | `editor_history.rs` の Record にまとめ、両方向 sync で Dirty フラグ |
| マルチカーソル/選択 | `Z:crates/editor/src/selections_collection.rs` | 当面シングルカーソル、必要時のみ拡張 |
| 診断オーバーレイ | `Z:crates/diagnostics/src/diagnostics.rs`, `Z:crates/diagnostics/src/buffer_diagnostics.rs`, `Z:crates/language/src/diagnostic.rs` | ガター entity に `⚠` を行単位で重ねる、Python は `rustpython-parser` か gRPC |
| Outline / シンボル | `Z:crates/outline/src/outline.rs`, `Z:crates/outline_panel/src/outline_panel.rs` | 将来、関数定義などのリスト表示が要るときに参照 |

### Picker / List / fuzzy 検索 (`src/ui/instrument_picker.rs`, `sidebar.rs` tickers)

| 機能 | Zed 参考 | 我々の翻訳先 |
|------|----------|--------------|
| ファジーマッチロジック | `Z:crates/fuzzy/src/matcher.rs`, `Z:crates/fuzzy/src/strings.rs`, `Z:crates/fuzzy_nucleo/` | `nucleo-matcher` クレートを直接、もしくは subsequence マッチ |
| picker UI (リスト + フィルタ + head) | `Z:crates/picker/src/picker.rs`, `Z:crates/picker/src/head.rs` | `bevy_egui::Window` + 仮想スクロール (`sidebar.rs` の Tickers を踏襲) |
| マッチハイライト表示 | `Z:crates/picker/src/highlighted_match_with_paths.rs` | egui の `RichText` で色分け、もしくは `Text2d` の `TextSection` 分割 |
| File finder | `Z:crates/file_finder/src/file_finder.rs`, `Z:crates/file_finder/src/file_finder_settings.rs` | `instrument_picker.rs` を雛形に拡張 |
| Tab switcher (Cmd+P 風) | `Z:crates/tab_switcher/src/tab_switcher.rs` | `bevy_egui` modal + 既存 floating window の上位 z-order |
| Recent projects | `Z:crates/recent_projects/src/recent_projects.rs` | `replay_startup_window.rs` / `scenario_startup_panel.rs` 拡張時の参考 |

### Command palette / Action / Menu / Keybinding

| 機能 | Zed 参考 | 我々の翻訳先 |
|------|----------|--------------|
| Command palette | `Z:crates/command_palette/src/command_palette.rs`, `Z:crates/command_palette_hooks/src/command_palette_hooks.rs` | picker パターン + action 列挙、起動キーは `KeyboardInput` で捕獲 |
| Action 定義 (型安全) | `Z:crates/zed_actions/src/lib.rs`, Zed の `actions!` マクロ | 1 アクション = 1 Bevy Event 型、`app.add_event::<MyAction>()` で配信 |
| Menu bar | `Z:crates/title_bar/src/title_bar.rs` のメニュー周り | `src/ui/menu_bar.rs` 既存パターン (ドロップダウン、Alt+F/E) を踏襲 |
| Modal layer (z-order) | `Z:crates/workspace/src/modal_layer.rs` | bevy_egui `Area::new(...).order(Order::Foreground)`、`FocusedWidget` を modal に向ける |
| Key binding | `Z:crates/settings/src/keymap_file.rs`, Zed `assets/keymaps/*.json` | 当面ハードコード `KeyboardInput` 判定、将来設定化なら参考 |

### Workspace / Panel / Layout / Persistence

| 機能 | Zed 参考 | 我々の翻訳先 |
|------|----------|--------------|
| Workspace 全体構造 | `Z:crates/workspace/src/workspace.rs` | `src/ui/window.rs` + `layout_persistence.rs` |
| Dockable panel | `Z:crates/workspace/src/dock.rs`, `Z:crates/workspace/src/pane.rs`, `Z:crates/workspace/src/pane_group.rs`, `Z:crates/panel/src/panel.rs` | `floating_window.rs` パターン拡張、drag-resize は Pointer observer |
| Project panel | `Z:crates/project_panel/src/project_panel.rs` | `src/ui/sidebar.rs` の section 拡張に参考 |
| Title bar / breadcrumbs | `Z:crates/title_bar/src/title_bar.rs`, `Z:crates/breadcrumbs/src/breadcrumbs.rs` | `src/ui/menu_bar.rs`, `footer.rs` |
| 永続化 | `Z:crates/workspace/src/persistence.rs` | `src/ui/layout_persistence.rs`、version 上げて migrate |
| Status bar | `Z:crates/workspace/src/status_bar.rs`, `Z:crates/activity_indicator/` | `src/ui/footer.rs` |
| Notification toast | `Z:crates/workspace/src/notifications.rs`, `Z:crates/workspace/src/toast_layer.rs`, `Z:crates/notifications/` | `bevy_egui` toast + Bevy `Timer` Component、最大件数で古い物から消す |
| Toolbar | `Z:crates/workspace/src/toolbar.rs` | 必要時 |

### Theme / Color / Settings

| 機能 | Zed 参考 | 我々の翻訳先 |
|------|----------|--------------|
| Theme schema | `Z:crates/theme/src/theme.rs`, `Z:crates/theme/src/default_colors.rs`, `Z:crates/theme/src/scale.rs` | `src/ui/components.rs` の色トークン定数群 |
| Theme selector | `Z:crates/theme_selector/src/theme_selector.rs` | 将来 dark/light 切替を作るときに |
| Syntax theme | `Z:crates/syntax_theme/src/syntax_theme.rs` | syntect の `Theme` をそのまま使う |
| Settings 永続化 | `Z:crates/settings/src/settings.rs` | 構造体 + JSON、`layout_persistence.rs` と同居 |
| Settings UI | `Z:crates/settings_ui/src/settings_ui.rs` | 将来、設定 modal を作るときに |

## Zed を読むときの読み方 (時間を浪費しない)

1. **対応表のファイルだけ開く**。Zed は 235 crates あるが、1 タスクで読むのは 1〜3 crate に絞る。
2. **`pub fn` のシグネチャだけ眺める** → 責務分割を掴む。中身の GPUI / Rope 呼び出しは無視。
3. **doc コメント (`///`) を読む** → Zed の rationale が出ている (なぜ rope か、なぜ MultiBuffer か、なぜ modal_layer を分けているか)。
4. **テスト (`*_tests.rs`) があれば先に読む** — edge case が入力例で並ぶ。例: `Z:crates/editor/src/editor_tests.rs` の auto-indent / bracket match、`Z:crates/picker/src/picker.rs` の selection 移動。
5. **GPUI 固有 API (`cx.notify()`, `View`, `Element`, `Subscription`, `cx.spawn`) は読み飛ばす** — 翻訳パターン表で置き換える。「どの state を変更したか」「どのイベントを発火したか」だけ抽出。
6. **圧倒されたら**: `crates/<domain>/src/lib.rs` から始めて `pub` exports を辿る。

## 翻訳パターン早見表

| Zed (GPUI) | 我々 (Bevy + cosmic_edit / egui) |
|------------|----------------------------------|
| `cx.subscribe(&buffer, \|this,_,event,cx\| ...)` | `EventReader<CosmicTextChanged>` または自前 Event |
| `cx.notify()` (再描画要求) | `buffer.set_redraw(true)` ＋ `Changed<T>` クエリ |
| `buffer.read(cx).text()` | `*Buffer.source: String` |
| `buffer.update(cx, \|b,cx\| b.edit(...))` | `editor.insert_string(s, None)` ＋ `Record::edit` push |
| `HighlightMap` / `SyntaxLayer` | `syntect::easy::HighlightLines` |
| `editor.scroll_position(cx)` | `editor.with_buffer(\|b\| b.scroll().vertical)` |
| `editor.selections.newest::<usize>(cx)` | `editor.cursor()` (シングルカーソル前提) |
| `Workspace::register_action(editor::Tab, ...)` | `KeyboardInput` EventReader + `Key::Tab` 判定 |
| `KeymapContext` (`vim_mode && editing`) | `FocusedWidget` Resource + 自前条件 |
| `div().child(line_numbers)` | 別 entity を spawn して content_area の子にする |
| Picker `PickerDelegate` trait | `Resource` に `candidates: Vec<T>` + `filter: String`、毎フレーム再フィルタ |
| `cx.spawn(async move {...})` | `bevy_tasks::AsyncComputeTaskPool` + Event で結果配信 |
| `cx.theme().colors().background` | `components.rs` 内の `pub const BG: Color = ...` |
| `actions!(mod_name, [DoFoo, DoBar])` | 1 アクション = 1 Bevy Event 型、`app.add_event::<DoFoo>()` |
| `ModalView` trait | `bevy_egui::Area::new(...).order(Order::Foreground)` + `FocusedWidget` を一時的に占有 |
| `cx.dispatch_action(Box::new(...))` | `EventWriter<MyAction>.send(MyAction)` |

## 必ず守る Caveat (Bevy + cosmic_edit 側の都合)

Zed のパターンを写しただけでは出てこない、Bevy/cosmic_edit/syntect 側の罠。Zed コードを読んだ後に必ず照合すること。

### bevy_cosmic_edit / cosmic_text 系
1. **CosmicEditor 内部 buffer 問題**  
   `set_rich_text` / 内容変更は `CosmicEditBuffer` と `CosmicEditor::with_buffer_mut` の**両方**に呼ぶ。focused フィールドは editor 内部 buffer を見るので、片方だけだと色や内容が反映されない。

2. **Sprite tint 問題**  
   ガター / スクロールバー / 通知トースト背景の `Sprite.color` を暗色にすると tint が乗って想定外。`Sprite.color = Color::WHITE` ＋ image / 別 Component で色管理。

3. **Tab / Enter キーの二重処理**  
   cosmic_edit の input system が同じキーを処理して二重挿入になる。`crates/bevy_cosmic_edit/src/input.rs` で Tab / Enter / メニュー用 Alt の default を無効化したうえで、我々の system だけが処理。

4. **行番号ガター・行内 widget のフォントメトリクス**  
   `Text2d` の `line_height` を editor 本体と完全一致させないと行がズレる。`EDITOR_LINE_HEIGHT` を共通定数化して両側で使う。

### syntect 系
5. **初期化コスト**  
   `SyntaxSet::load_defaults_newlines()` は数十〜数百ms。**Startup で 1 回**だけ実行して `Highlighter` Resource に保持。Update で呼ぶと毎フレーム重い。

6. **ハイライトの重ね順 (system ordering)**  
   構文色 → Find マッチ → ブラケットマッチ → 診断、の順で `after()`。逆順だと上書きされて消える。

7. **`set_rich_text` の存在確認**  
   bevy_cosmic_edit / cosmic_text のバージョンで API シグネチャが変わる。`crates/bevy_cosmic_edit/` を先に Grep。なければ `set_line_attributes` / `BufferLine::set_attrs_list` でフォールバック。

### Undo/Redo / 双方向 sync 系
8. **Undo/Redo 後のハイライト再適用**  
   `history.replaying = true` の間は `sync_editor_to_*_system` の `Record::edit` push をスキップするが、**ハイライト/フィルタ等の派生 system はスキップしない**。Undo/Redo 後こそ Dirty フラグを立てて再計算する。`sync_*_to_editor_system` 側で Dirty 立て。

### Picker / Modal / 通知トースト系
9. **bevy_egui modal の z-order**  
   floating_window より上に出すには `egui::Order::Foreground` + `Area` で明示。`anchor` を使うとレイアウトが崩れる場合あり。Zed の `modal_layer.rs` の責務 = 「他の入力をブロックする層」を意識して、open 中は背後 entity への click/keyboard を遮断。

10. **picker のフォーカス遷移**  
    `FocusedWidget` Resource を modal open 時に textfield に向け、close 時に元 entity に戻す。**戻し忘れるとキー入力が無効化される**。Zed の `PickerDelegate::dismissed` と同じタイミング。

11. **通知トーストの寿命管理**  
    toast 1 件 = 1 entity (Bevy `Timer` Component 持ち)、`Timer::finished()` で despawn。最大件数を超えたら古い物から `Commands::entity(_).despawn()`。Zed `toast_layer.rs` の queue 管理 = 我々ではシンプルに `Query<&Timer, With<Toast>>` で十分。

### Layout / 永続化 / z-order
12. **layout_persistence の version**  
    fields を増減したら version を上げて旧 JSON を migrate / 捨てる。互換性を雑にやると起動時 panic。Zed の `persistence.rs` の sqlite migration と同じ責務だが、我々は JSON + version int で OK。

13. **z-order と pickability**  
    Bevy で Sprite/Text を重ねる場合、`Transform.translation.z` ＋ `Pickable` の組み合わせで back-to-front 制御。重なってると click が通らない事故あり。modal は最前面 z + 背後 `Pickable::IGNORE`。

## 推奨ワークフロー (UI 機能 1 単位 ≒ 1 turn)

新規 UI 機能 / 既存改修 1 件につき:

1. **ドメインを上の対応表から特定** → 該当 Zed crate を 1〜3 個 Read (`pub fn` とコメントと `*_tests.rs`)。5 分で十分。
2. **我々の既存隣接コードを Read** (`src/ui/<near-by>.rs`) — 再利用ポイント・既存パターンを特定。重複実装を避ける。
3. **設計を 3-5 行で要約してからコードを書く** — どの Component / Resource / system / Event を足すか、既存のどれを再利用するか。
4. **Caveat 一覧と照合** — 特に 1, 3, 5, 6, 7, 8, 10 はエディタ + modal 系で毎回踏みうる。
5. **実装 → `cargo check` → `cargo test --lib` → 目視 E2E** (E2E は `e2e-testing` スキル併用)。
6. **長丁場 (複数ファイル / 複数 phase) になりそうなら `pair-relay` スキルへ移行**、本スキルの該当節を Navigator に引き継ぐ。
7. **完了したら `post-impl-skill-update` スキル発動** — 本スキルの description / 対応表 / Caveat を実情にアップデート。

## このスキルの対象になっている src/ui/* (現状スナップショット)

- `strategy_editor.rs` — Phase 7.2 進行中。**Phase A 完了**: syntax highlight (syntect 全文再トークナイズ) + bracket match + Layer Composer (`strategy_editor_highlight.rs` / `strategy_editor_compose.rs`、span source を固定順で合成し `apply_highlight_layers_system` だけが `set_attrs_list` を呼ぶ)。**Phase E 完了**: find/replace (`strategy_editor_find.rs`)。Find マッチは composer 経由で塗り (`FindMatchSpans` を書くだけ・`set_attrs_list` は呼ばない)、replace は純粋関数 `apply_replacement` で新ソースを計算し `editor.set_text` + `CosmicTextChanged` で既存パイプライン (fragment→undo→autosave→再ハイライト) に丸投げ。Find パネルは `spawn_floating_window` 再利用 + 専用マーカー `FindQueryEditor`/`FindReplacementEditor` (`StrategyEditorContent` は付けない) + `panel_root: Option<Entity>` lifecycle。**Phase B 完了**: 行番号 gutter (`strategy_editor_gutter.rs`) + scrollbar (`strategy_editor_scrollbar.rs`)。**Phase C 完了**: Tab→4 spaces / Enter→auto-indent / bracket autoclose (`strategy_editor_input.rs`)。新パネルや span source を足すときは composer の固定順序 (default→syntax→find→current_find→bracket) を踏襲する。⚠️ 世界空間の小型 cosmic 入力欄 (find query/replacement 等) は Sprite 高さを line_height の DPI 2x ダブリング(18→36)より大きく (44px 等) しないと retina で glyph が出ない (bevy-engine DPI トラップ)。
  - **Phase B gutter の罠**: gutter は別 `CosmicEditBuffer` だが **`TextEdit2d` を外すと描画されない** (このフォークの `CosmicWidgetSize::scan()` が `Has<TextEdit2d>` を要求。計画書の「TextEdit2d を外せば入力除外」は誤り)。代わりに **`ReadOnly`** を付ける → `change_active_editor_sprite` (`Without<ReadOnly>` フィルタ) の focus-on-click 対象外になり編集も無効化。`StrategyEditorContent` は付けない (highlight/sync 系から自動除外)。行高一致は `editor_metrics()` 共通 Metrics、focused/unfocused の scroll 読みは `read_active_buffer()` ヘルパで統一。
  - **wrap は `CosmicWrap` Component**: 計画書の `Buffer::set_wrap(Wrap::None)` はこのフォークに無い。`CosmicWrap::InfiniteLine` を editor/gutter spawn tuple に入れるだけで折返し無効 (source 行 == layout 行 → gutter 行番号と一致)。
  - **Phase C input の罠**: `InputSet` は `bevy_cosmic_edit::InputSet` (crate root。`input` module は private で `input::InputSet` は不可)。keyboard 系 (`kb_input_text` 等) は **Update** の InputSet で走る (`input_mouse` だけ PreUpdate) ので Update system から `.before/.after(InputSet)` が効く。Tab/Enter は `.before(InputSet)` + `ResMut<ButtonInput<KeyCode>>::reset(key)`、bracket closer は `.after(InputSet)` + `Events::clear()` 禁止。カスタム編集は `CosmicTextChanged` を手動 send 必須 (cosmic は `is_edit` のときしか発火しない)。全文復元の `BufferExtras::get_text` は `pub(crate)` で src/ から呼べない → `b.lines.iter().map(|l| l.text()).collect::<Vec<_>>().join("\n")` で代替。
  - **Resource が別 entity の panel ハンドルを持つときの孤児リセット (Phase 7.2 review で発覚した Medium バグ)**: `FindReplaceState` のように lifecycle system (`manage_find_panel_lifecycle_system`) が `panel_root: Option<Entity>` を見て despawn する設計では、孤児チェック (target editor が × で消えた等) で **`*state = FindReplaceState::default()` を素で呼ぶと panel_root の Entity 参照ごと捨ててしまい、Find パネル本体 (別 entity) が永久に despawn されず画面にリークする** (close 分岐は `panel_root.is_some()` を要求するので発火しない)。リセット時は `panel_root` / `query_editor` / `replacement_editor` を退避して `default()` 後に書き戻し、`is_open=false` で close 遷移に despawn を委ねる。
  - **2 つの Update system が同じキーを奪い合うと非決定 (Phase 7.2 review)**: `enter_autoindent_system` (`.before(InputSet)` で Enter→改行) と `find_navigate_system` (Enter→次マッチ) のように、別々の `add_systems` 呼び出しに属し相互の `.before/.after` が無い 2 system が同じ `keys.just_pressed(Enter)` を読むと、`keys.reset` の効く順序がフレームごとに揺れて「改行 + ナビ」二重発火になる。同一キーの分岐は focus 条件 (`focused.0 == state.target_editor` 等) で**重ならないよう排他化**するか、明示的に `.before/.after` で順序固定する。
- `instrument_picker.rs` — picker パターンの既存実装
- `sidebar.rs` — virtual scroll + tickers リストの既存実装
- `floating_window.rs` / `layout_persistence.rs` — workspace / persistence パターン
- `menu_bar.rs` / `footer.rs` — title_bar / status_bar 相当
- `chart.rs` / `orders.rs` / `positions.rs` / `buying_power.rs` / `run_result_panel.rs` — table / list / panel パターン (Zed の `editor_benchmarks` ではないが pane の概念は流用可)
- `scenario_startup_panel.rs` / `replay_startup_window.rs` — recent_projects / welcome 系の参考になる側

新機能を作るときはまずこれら**隣接ファイルを Read してから Zed を読む**。重複実装を避けるため。

## 他スキルとの境界 (いつ切り替えるか)

- Bevy ECS / WGPU / Asset / `add_systems` 20 上限・observer・required components の一般論 → **`bevy-engine`** スキル併用必須
- バックエンド (Python/gRPC、nautilus_trader、戦略実行) → **`nautilus_trader`** / **`tachibana`** / **`kabusapi`** スキルへ
- Rust テスト一般 (`#[cfg(test)] mod tests`、Bevy `App` テスト、`serial_test`、env var) → **`rust-testing`** スキル併用
- Python テスト一般 (pytest / pytest-httpx / freezegun) → **`tdd-workflow`** スキル
- E2E 手動検証 (`backcast.exe` + `python -m engine`、レイアウトの目視) → **`e2e-testing`** スキル
- 大規模並列実装 (5 タスク以上、依存解決可能) → **`parallel-agent-dev`** スキル
- 長丁場の段階実装 (TDD・複数ファイル・複数レイヤー) → **`pair-relay`** スキル (Navigator spawn 前に本スキル invoke 必須)
- 学習目的・ユーザがドライバー → **`pair-nav`** スキル
- 実装完了/コミット/フェーズ終了 → **`post-impl-skill-update`** スキル (CLAUDE.md 必須ルール)
- 変更コードのレビュー (再利用・品質・効率) → **`simplify`** スキル
