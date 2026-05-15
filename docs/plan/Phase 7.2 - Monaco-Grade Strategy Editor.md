# Phase 7.2 — Monaco-Grade Strategy Editor

## Overview

Strategy Editor を Monaco Editor 相当に昇華させる。  
現在は「テキストを開いて保存・実行できるだけ」の最小エディタ。  
このフェーズで **Python 専用コードエディタ** としての体験を揃える。

**前提: Phase 7.1 (Undo/Redo) が完了していること。**  
`EditorHistory` Resource・`replaying` フラグ・`undo_redo_system` が実装済みである前提で設計する。

---

## 現在地 (Phase 7.1 完了後)

| 項目 | 状態 |
|------|------|
| Undo/Redo | ✅ Phase 7.1 で実装済み (`EditorHistory`, `undo_redo_system`) |
| シンタックスハイライト | 未実装 — 全文 `rgb(220, 220, 220)` |
| 行番号 | 未実装 |
| ステータスバー | 未実装 |
| スクロールバー | 未実装 |
| オートインデント / Tab | 未実装 |

**主要ファイル:**

- [src/ui/strategy_editor.rs](../../src/ui/strategy_editor.rs) — エディタパネル本体
- [src/ui/components.rs](../../src/ui/components.rs) — `StrategyBuffer`, `EditorHistory`
- [src/ui/editor_history.rs](../../src/ui/editor_history.rs) — `TextEdit` (Phase 7.1 で追加)
- [crates/bevy_cosmic_edit/](../../crates/bevy_cosmic_edit/) — ローカルフォーク

---

## Target: Monaco 相当の機能セット

| # | 機能 | 優先度 |
|---|------|--------|
| 0 | **Undo/Redo との統合** (Phase 7.1 成果物の配線) | P0 |
| 1 | **Python シンタックスハイライト** | P0 |
| 2 | **行番号ガター** | P0 |
| 3 | **ステータスバー** (行/列 + 文字数) | P1 |
| 4 | **スクロールバー** (縦) | P1 |
| 5 | **オートインデント** (Enter → インデント継承、`:` の後 +4sp) | P1 |
| 6 | **Tab → 4 スペース** | P1 |
| 7 | **Find & Replace** (Ctrl+F / Ctrl+H) | P2 |
| 8 | **ブラケットマッチング** (`(` → `)` 強調) | P2 |
| 9 | **エラーインジケーター** (Python 構文チェック → 赤波線) | P3 |

---

## アーキテクチャ決定

### シンタックスハイライト方式

`cosmic_text` の `Buffer::set_rich_text` は **スパン単位で `Attrs` (色) を指定できる**。  
これを利用して、テキスト変更のたびに全文を再トークナイズして色付き spans を書き込む。

トークナイザは **[`syntect`](https://crates.io/crates/syntect)** を使用:
- Python 文法が同梱されている
- `HighlightLines` が `(style, text)` のイテレータを返す
- `style.foreground` を `CosmicColor` に変換して span ごとに `Attrs::color()` を付ける

```
CosmicTextChanged イベント / Undo-Redo 適用
    → SyntaxDirty = true
    → apply_syntax_highlight_system
        → highlight_python(source) -> Vec<(&str, AttrsOwned)>
        → Buffer::set_rich_text(spans)
        → set_redraw(true)
```

> **Caveat**: `set_rich_text` 後は `CosmicEditor` (フォーカス中) 内部の buffer にも同様に適用が必要  
> (memory #4545 参照: `CosmicEditor` が存在する間、render は editor 内部 buffer を見る)

### Undo/Redo との接続

Phase 7.1 の `sync_strategy_buffer_to_editor_system` は Undo/Redo 適用後にも呼ばれる。  
この system 内で **`syntax_dirty.0 = true`** をセットすることで、  
Undo/Redo で戻したテキストにもシンタックスハイライトが自動再適用される。

```
undo_redo_system
    → buffer.source を書き換え
    → sync_strategy_buffer_to_editor_system  (OpenStrategyRequested / UndoRedoApplied)
        → syntax_dirty.0 = true
    → apply_syntax_highlight_system
```

`history.replaying = true` の間は `apply_syntax_highlight_system` をスキップしない  
（undo/redo 後こそハイライトを再適用する必要がある）。  
スキップが必要なのは `sync_editor_to_strategy_buffer_system` 側の `Record::edit` push だけ。

### 行番号ガター

エディタ entity の **左隣に Text2d ガター entity** を spawn する。  
毎フレーム `StrategyBuffer` の行数 + エディタのスクロール量を読んで行番号テキストを更新。

```
content_area
├── gutter_entity  (Text2d, width=50px, 右寄せ数字)
└── editor_entity  (TextEdit2d, 左 offset 50px)
```

ガター幅は行数に応じて動的: `ceil(log10(line_count + 1)) * char_width + padding`

### Find & Replace

`bevy_egui` の `egui::Window` を Strategy Editor の上に重ねてフローティング表示。  
Ctrl+F で表示/非表示を切り替える。Replace は Ctrl+H。

```
FindReplaceState (Resource)
  open: bool
  mode: Find | Replace
  query: String
  replacement: String
  case_sensitive: bool
  match_positions: Vec<usize>  // byte offsets in source
  current_match: usize
```

マッチ部分は `CosmicColor` で黄色ハイライト → シンタックスハイライトに重ねる。

### ステータスバー

エディタ下部に固定高さ (20px) の `Text2d` entity を置く。  
毎フレームカーソル位置 (行/列) と文字数を表示。  
Undo/Redo で戻った後も `buffer.source` が更新されるので自動的に反映される。

### スクロールバー

エディタ右端に `Sprite` entity でバー表示。
- トラック高さ = エディタ高さ
- サム高さ = `(visible_lines / total_lines) * track_height`
- サム位置 = `(scroll_offset / max_scroll) * (track_height - thumb_height)`

### オートインデント

`KeyboardInput` イベントを監視する system を追加。  
`Key::Enter` が押された時点で editor が focused なら:
1. カーソル行の先頭スペース数を数える
2. 直前行が `:` で終わるなら +4 を足す
3. `\n` + indent_str をバッファに書き込む

---

## 実装ステップ

### Sub-step 7.2.0 — Undo/Redo との統合配線

**Phase 7.1 の成果物を Phase 7.2 の新 systems に接続する。**

`sync_strategy_buffer_to_editor_system` に `SyntaxDirty` のセットを追加:

```rust
pub fn sync_strategy_buffer_to_editor_system(
    mut events: EventReader<OpenStrategyRequested>,
    buffer: Res<StrategyBuffer>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut syntax_dirty: ResMut<SyntaxDirty>,   // ← 追加
    mut editor_q: Query<...>,
) {
    if events.is_empty() { return; }
    events.clear();
    // ... 既存の set_text 処理 ...
    syntax_dirty.0 = true;   // ← 追加: Undo/Redo 後もハイライトを再適用
}
```

`sync_editor_to_strategy_buffer_system` にも追加:

```rust
pub fn sync_editor_to_strategy_buffer_system(
    mut events: EventReader<CosmicTextChanged>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut buffer: ResMut<StrategyBuffer>,
    mut history: ResMut<EditorHistory>,
    mut syntax_dirty: ResMut<SyntaxDirty>,   // ← 追加
) {
    for CosmicTextChanged((entity, new_text)) in events.read() {
        if !editor_q.contains(*entity) { continue; }
        if buffer.source == *new_text { continue; }
        if history.replaying { continue; }
        // ... 既存の Record::edit 処理 ...
        syntax_dirty.0 = true;   // ← 追加
    }
}
```

`SyntaxDirty` Resource は Sub-step 7.2.3 で定義するが、  
この配線だけ先に入れておき `cargo check` を通す。

---

### Sub-step 7.2.1 — Cargo.toml に syntect を追加

```toml
[dependencies]
syntect = { version = "5", default-features = false, features = ["default-syntaxes", "default-themes", "fancy-regex"] }
```

`fancy-regex` バックエンドを使うことで Windows で `onig` (C ライブラリ) への依存を回避する。

**`cargo check` でビルドが通ることを確認すること。**

---

### Sub-step 7.2.2 — `src/highlight.rs` を新規作成

```rust
// src/highlight.rs
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Color as CosmicColor};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

#[derive(Resource)]
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Python ソースを解析し (text_fragment, AttrsOwned) のリストを返す。
    /// `Buffer::set_rich_text` に渡す形式。
    pub fn highlight_python<'a>(&self, source: &'a str) -> Vec<(&'a str, AttrsOwned)> {
        let syntax = self
            .syntax_set
            .find_syntax_by_extension("py")
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let theme = &self.theme_set.themes["Monokai Extended"];
        let mut h = HighlightLines::new(syntax, theme);
        let mut spans = Vec::new();

        for line in LinesWithEndings::from(source) {
            let ranges = h.highlight_line(line, &self.syntax_set).unwrap_or_default();
            for (style, text) in ranges {
                let color = style_to_cosmic(style);
                spans.push((text, AttrsOwned::new(Attrs::new().color(color))));
            }
        }
        spans
    }
}

fn style_to_cosmic(style: Style) -> CosmicColor {
    let c = style.foreground;
    CosmicColor::rgba(c.r, c.g, c.b, c.a)
}
```

`Startup` system で Resource 登録:

```rust
app.insert_resource(Highlighter::new());
```

> **注意**: `SyntaxSet::load_defaults_newlines()` は初期化に数十〜数百ms かかる。  
> `Startup` で一度だけ実行すること。`Update` の毎フレームで呼ばない。

---

### Sub-step 7.2.3 — シンタックスハイライト適用 system

`SyntaxDirty` フラグを追加し、変更があったフレームだけ再トークナイズする。

```rust
// src/ui/components.rs に追加
#[derive(Resource, Default)]
pub struct SyntaxDirty(pub bool);
```

`apply_syntax_highlight_system`:

```rust
pub fn apply_syntax_highlight_system(
    mut syntax_dirty: ResMut<SyntaxDirty>,
    buffer: Res<StrategyBuffer>,
    highlighter: Res<Highlighter>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut editor_q: Query<
        (&mut CosmicEditBuffer, Option<&mut CosmicEditor>),
        With<StrategyEditorContent>,
    >,
) {
    if !syntax_dirty.0 {
        return;
    }
    syntax_dirty.0 = false;

    let spans = highlighter.highlight_python(&buffer.source);

    for (mut edit_buffer, editor_opt) in &mut editor_q {
        edit_buffer.set_rich_text(
            &mut font_system,
            spans.iter().map(|(t, a)| (*t, a.as_attrs())),
            Attrs::new(),
            Shaping::Advanced,
        );

        // CosmicEditor が居ればそちらも更新 (memory #4545 参照)
        if let Some(mut editor) = editor_opt {
            editor.with_buffer_mut(|b| {
                b.set_rich_text(
                    spans.iter().map(|(t, a)| (*t, a.as_attrs())),
                    Attrs::new(),
                    Shaping::Advanced,
                );
                b.set_redraw(true);
            });
        }
    }
}
```

**system 順序:**

```
sync_editor_to_strategy_buffer_system  (sets SyntaxDirty)
sync_strategy_buffer_to_editor_system  (sets SyntaxDirty — Undo/Redo 後も含む)
    → apply_syntax_highlight_system    (after 両方)
```

---

### Sub-step 7.2.4 — 行番号ガター entity

`spawn_strategy_editor_panel` を修正し、`content_area` の中に gutter と editor を並べる。

```
content_area (500 × 380)
├── gutter_entity   Text2d, size=(50, 360), pos=(-195, 10), Z=0.15
│     右寄せ 1-indexed 行番号、薄いグレー rgb(100,100,120)
└── editor_entity   TextEdit2d, size=(440, 360), pos=(-5, 10), Z=0.1
      ← 元の EDITOR_SIZE から左端を 50px 詰めた位置
```

ガター更新 system:

```rust
#[derive(Component)]
pub struct LineNumberGutter;

pub fn update_line_number_gutter_system(
    buffer: Res<StrategyBuffer>,
    mut gutter_q: Query<&mut Text2d, With<LineNumberGutter>>,
) {
    if !buffer.is_changed() {
        return;
    }
    let line_count = buffer.source.lines().count().max(1);
    let text = (1..=line_count)
        .map(|n| format!("{:>4}", n))
        .collect::<Vec<_>>()
        .join("\n");
    for mut t in &mut gutter_q {
        t.0 = text.clone();
    }
}
```

`buffer.is_changed()` は Undo/Redo 後にも `true` になるため、  
行番号は自動的に再描画される。フォントメトリクス (行高) は `EDITOR_LINE_HEIGHT` と揃えること。

---

### Sub-step 7.2.5 — ステータスバー

パネル下部に 20px の `Text2d` entity を追加する。

```
content_area
└── status_bar_entity  Text2d, size=(500, 20), Z=0.15
      pos = (0, -(PANEL_SIZE.y / 2 - 10))
      テキスト例: "Ln 42, Col 8  |  1,234 chars  |  Python"
```

```rust
#[derive(Component)]
pub struct StatusBar;

pub fn update_status_bar_system(
    buffer: Res<StrategyBuffer>,
    history: Res<EditorHistory>,
    editor_q: Query<Option<&CosmicEditor>, With<StrategyEditorContent>>,
    mut status_q: Query<&mut Text2d, With<StatusBar>>,
) {
    let Ok(editor_opt) = editor_q.get_single() else { return; };
    let (line, col) = editor_opt
        .map(|e| {
            let c = e.cursor();
            (c.line + 1, c.index + 1)
        })
        .unwrap_or((1, 1));
    let chars = buffer.source.chars().count();
    let undo_indicator = if history.record.can_undo() { "↩" } else { " " };
    for mut t in &mut status_q {
        t.0 = format!("{undo_indicator} Ln {line}, Col {col}  |  {chars} chars  |  Python");
    }
}
```

Undo 可能なときは `↩` をステータスバーに表示することで Undo 状態をユーザーに伝える。

---

### Sub-step 7.2.6 — Tab キー → 4 スペース

`KeyboardInput` system で `Key::Tab` を横取りし、4 スペースを挿入する。

```rust
pub fn tab_to_spaces_system(
    mut key_events: EventReader<KeyboardInput>,
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, Option<&mut CosmicEditor>), With<StrategyEditorContent>>,
) {
    for event in key_events.read() {
        if event.state != ButtonState::Pressed { continue; }
        if event.logical_key != Key::Tab { continue; }
        let Some(focused_entity) = focused.0 else { continue; };
        for (entity, editor_opt) in &mut editor_q {
            if entity != focused_entity { continue; }
            if let Some(mut editor) = editor_opt {
                editor.insert_string("    ", None);
                editor.with_buffer_mut(|b| b.set_redraw(true));
            }
        }
    }
}
```

> **注意**: Tab イベントを横取りした場合、cosmic_edit 側でも処理されると二重になる可能性がある。  
> local fork `crates/bevy_cosmic_edit/src/input.rs` で Tab の default 処理を無効化するパッチを当てること。

---

### Sub-step 7.2.7 — オートインデント

Enter キー押下時にカーソル前行のインデントを引き継ぐ。

```rust
pub fn auto_indent_system(
    mut key_events: EventReader<KeyboardInput>,
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, Option<&mut CosmicEditor>), With<StrategyEditorContent>>,
) {
    for event in key_events.read() {
        if event.state != ButtonState::Pressed { continue; }
        if event.logical_key != Key::Enter { continue; }
        let Some(focused_entity) = focused.0 else { continue; };
        for (entity, editor_opt) in &mut editor_q {
            if entity != focused_entity { continue; }
            if let Some(mut editor) = editor_opt {
                let indent = compute_next_indent(&editor);
                editor.insert_string(&indent, None);
                editor.with_buffer_mut(|b| b.set_redraw(true));
            }
        }
    }
}

fn compute_next_indent(editor: &CosmicEditor) -> String {
    let cursor = editor.cursor();
    let line_idx = cursor.line.saturating_sub(1); // Enter 後は 1 行進んでいる
    let line = editor.with_buffer(|b| {
        b.lines.get(line_idx).map(|l| l.text().to_string())
    });
    let Some(line) = line else { return String::new(); };
    let base_indent = line.len() - line.trim_start().len();
    let extra = if line.trim_end().ends_with(':') { 4 } else { 0 };
    " ".repeat(base_indent + extra)
}
```

---

### Sub-step 7.2.8 — スクロールバー (P1)

エディタ右端に 8px 幅の縦スクロールバーを `Sprite` entity で追加。

```rust
#[derive(Component)]
pub struct EditorScrollThumb;

pub fn update_scrollbar_system(
    editor_q: Query<Option<&CosmicEditor>, With<StrategyEditorContent>>,
    buffer: Res<StrategyBuffer>,
    mut thumb_q: Query<&mut Transform, With<EditorScrollThumb>>,
) {
    let total_lines = buffer.source.lines().count().max(1) as f32;
    let visible_lines = (EDITOR_SIZE.y / EDITOR_LINE_HEIGHT).floor();
    let max_scroll = (total_lines - visible_lines).max(0.0);

    let scroll_pos = editor_q.get_single().ok()
        .flatten()
        .map(|e| e.with_buffer(|b| b.scroll().vertical as f32))
        .unwrap_or(0.0);

    let ratio = if max_scroll > 0.0 { scroll_pos / max_scroll } else { 0.0 };
    let track_height = EDITOR_SIZE.y;
    let thumb_height = (visible_lines / total_lines * track_height).max(20.0);
    let thumb_y = (track_height / 2.0 - thumb_height / 2.0)
        - ratio * (track_height - thumb_height);

    for mut t in &mut thumb_q {
        t.translation.y = thumb_y;
    }
}
```

---

### Sub-step 7.2.9 — Find & Replace パネル (P2)

`FindReplaceState` Resource を追加し、bevy_egui の `egui::Window` で UI を描画する。

```rust
#[derive(Resource, Default)]
pub struct FindReplaceState {
    pub open: bool,
    pub replace_mode: bool,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    /// buffer.source 内のマッチ開始バイトオフセット一覧
    pub matches: Vec<usize>,
    pub current: usize,
}
```

Keyboard shortcut system:

```rust
pub fn find_replace_shortcut_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<FindReplaceState>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if ctrl && keys.just_pressed(KeyCode::KeyF) {
        state.open = !state.open;
        state.replace_mode = false;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyH) {
        state.open = true;
        state.replace_mode = true;
    }
    if keys.just_pressed(KeyCode::Escape) {
        state.open = false;
    }
}
```

Find 結果のハイライトは `apply_syntax_highlight_system` の後段で  
マッチ範囲の `Attrs` を黄色で上書きする方式で実現する。

---

## エラーインジケーター (P3 — 将来フェーズ)

| 方式 | 概要 | 難度 |
|------|------|------|
| `rustpython-parser` crate | Rust 内で AST パース | ★★☆ |
| gRPC `CheckSyntax` RPC | Python backend に `ast.parse` させる | ★★★ |

エラー行番号が返ってきたら、ガター entity の該当行に `⚠` または赤背景を付ける。

---

## system 登録順序 (UiPlugin)

```rust
app
    .init_resource::<SyntaxDirty>()
    .init_resource::<FindReplaceState>()
    .insert_resource(Highlighter::new())   // Startup 相当
    .add_systems(Update, (
        // Phase 7.1 からの既存 systems
        undo_redo_system
            .after(sync_editor_to_strategy_buffer_system),
        // Phase 7.2 新規
        sync_strategy_buffer_to_editor_system
            .after(open_strategy_buffer_system)
            .after(undo_redo_system),
        sync_editor_to_strategy_buffer_system,
        apply_syntax_highlight_system
            .after(sync_editor_to_strategy_buffer_system)
            .after(sync_strategy_buffer_to_editor_system),
        update_strategy_editor_zoom_system,
        update_strategy_button_visuals_system,
        update_line_number_gutter_system,
        update_status_bar_system,
        update_scrollbar_system,
        tab_to_spaces_system,
        auto_indent_system,
        find_replace_shortcut_system,
        find_replace_ui_system,
    ));
```

---

## ファイル変更一覧

| ファイル | 変更種別 | 内容 |
|----------|----------|------|
| `Cargo.toml` | 追記 | `syntect = "5"` (fancy-regex feature) |
| `src/highlight.rs` | 新規 | `Highlighter` resource, `highlight_python` |
| `src/ui/strategy_editor.rs` | 変更 | ガター・ステータスバー・スクロールバー spawn; 新 systems 追加; `SyntaxDirty` セット追加 |
| `src/ui/components.rs` | 変更 | `SyntaxDirty`, `FindReplaceState` resource 追加 |
| `src/ui/find_replace.rs` | 新規 | Find/Replace egui window system |
| `src/lib.rs` または `src/main.rs` | 変更 | `mod highlight;` 追加, `Highlighter` 登録 |

---

## 既知 Caveat (実装前に必ず確認)

1. **CosmicEditor 内部 buffer 問題** (memory #4545)  
   シンタックスハイライト適用は `CosmicEditBuffer` と `CosmicEditor` 両方に `set_rich_text` を呼ぶ。

2. **Sprite.color tint 問題** (memory #4527)  
   ガター / スクロールバーに暗色 Sprite を使う場合は `CosmicBackgroundColor` で背景を指定し、  
   `Sprite.color = Color::WHITE` にする。

3. **Tab/Enter キーの二重処理**  
   `tab_to_spaces_system` / `auto_indent_system` が cosmic_edit の input system と競合する可能性。  
   local fork `crates/bevy_cosmic_edit/src/input.rs` で Tab/Enter の default 処理を無効化すること。

4. **syntect の初期化コスト**  
   `SyntaxSet::load_defaults_newlines()` は重い。`Startup` で一度だけ実行し Resource に持つ。

5. **set_rich_text API の存在確認**  
   bevy_cosmic_edit 0.26 / cosmic_text の `Buffer::set_rich_text` が存在するか確認すること。  
   存在しない場合、行単位で `set_line_attributes` するフォールバックが必要。

6. **Undo/Redo 後のシンタックスハイライト**  
   `undo_redo_system` → `sync_strategy_buffer_to_editor_system` → `syntax_dirty = true`  
   の順で必ず流れることを system ordering で保証すること。

---

## 完了条件

- [ ] Python ファイルを開いたとき keyword/string/comment が色分けされる
- [ ] Ctrl+Z で Undo した後もシンタックスハイライトが正しく再適用される
- [ ] 行番号がエディタ左端に表示される (スクロールしても追従)
- [ ] 下部ステータスバーにカーソル行/列が表示される (Undo 後も更新される)
- [ ] Tab キーで 4 スペースが挿入される
- [ ] Enter キーで直前行のインデントが継承される (`:` 後は +4)
- [ ] Ctrl+F で Find パネルが開き、文字列を強調表示できる
- [ ] 縦スクロールバーがスクロール量に応じて動く
- [ ] `cargo check` / `cargo test` が通る
- [ ] E2E: test_strategy_daily.py を開いて色付き → 編集 → Ctrl+Z → 色が再適用される

---

## 実装順序の推奨

```
7.2.0  Undo/Redo 配線 (SyntaxDirty セット追加) — cargo check
    → 7.2.1  syntect Cargo.toml 追加
    → 7.2.2  highlight.rs + Highlighter resource
    → 7.2.3  SyntaxDirty + apply_syntax_highlight_system (E2E 目視)
    → 7.2.4  行番号ガター
    → 7.2.5  ステータスバー (Undo インジケーター込み)
    → 7.2.6  Tab → 4sp
    → 7.2.7  オートインデント
    → 7.2.8  スクロールバー
    → 7.2.9  Find & Replace (最後)
```

シンタックスハイライトが最大の視覚的インパクト + Undo/Redo との結合検証を  
Sub-step 7.2.3 終了時点で行うこと。
