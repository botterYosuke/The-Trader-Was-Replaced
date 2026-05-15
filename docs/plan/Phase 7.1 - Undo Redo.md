# Phase 7.2 — Undo / Redo

## Overview

Strategy Editor に **Ctrl+Z / Ctrl+Y** (Undo/Redo) を実装する。  
`bevy_cosmic_edit 0.26` は CHANGELOG で明示的に Undo/Redo を削除しており、  
自前実装が必要。[`undo`](https://crates.io/crates/undo) crate v0.52 を採用する。

---

## 現在地 (2026-05-15 時点)

| 項目 | 状態 |
|------|------|
| ブランチ | `feature/7.6-UI-Resilient-Diffie` |
| Undo/Redo | **未実装** — cosmic_edit から削除済み |
| テキスト変更イベント | `CosmicTextChanged(Entity, String)` で全文が届く |

**関連ファイル:**

- [src/ui/strategy_editor.rs](../../src/ui/strategy_editor.rs) — `sync_editor_to_strategy_buffer_system`
- [src/ui/components.rs](../../src/ui/components.rs) — `StrategyBuffer`
- [Cargo.toml](../../Cargo.toml) — dependency 追加対象

---

## 採用 crate: `undo` v0.52

```toml
[dependencies]
undo = "0.52"
```

### なぜ `undo` か

| crate | 採用理由 / 見送り理由 |
|-------|----------------------|
| **`undo` v0.52** | 活発メンテ・`merge()` あり・Bevy 非依存・`Record<String>` で即使える |
| `undo_2` v0.2.1 | 開発停滞、API が低レベル過ぎる |
| 手書きスナップショット | merge なし・上限管理が面倒 |

### 主要 API

```rust
use undo::{Edit, Record};

// Record<Target> — Target は undo が管理する状態の型
let mut record: Record<String> = Record::new();

// 変更を push
record.edit(&mut target, MyEdit { before, after });

// Ctrl+Z
record.undo(&mut target);

// Ctrl+Y
record.redo(&mut target);
```

---

## 設計方針

### State: `StrategyBuffer.source` をターゲットにする

`undo::Record` のターゲット型を `String` (= `StrategyBuffer.source`) とする。  
Bevy の `Resource` に包んで管理する。

```
StrategyBuffer.source  ←→  EditorHistory (Record<String>)
```

`CosmicTextChanged` イベントが来るたびに `Record::edit` へ push。  
Ctrl+Z で `record.undo(&mut source)` → `sync_strategy_buffer_to_editor_system` 経由でエディタに反映。

### Merge: 連続入力を 1 undo にまとめる

Monaco は「単語境界」または「一定時間の無操作」でひとつの undo 単位とする。  
`undo::Edit::merge` を実装し、**前の編集から 500ms 以内かつ単語境界でない** 場合はマージする。

```rust
fn merge(&mut self, other: &Self) -> undo::Merge {
    if self.timestamp.elapsed() < Duration::from_millis(500)
        && !self.after.ends_with(' ')
        && !self.after.ends_with('\n')
    {
        self.after = other.after.clone();
        undo::Merge::Yes
    } else {
        undo::Merge::No
    }
}
```

### 上限

デフォルトは無制限。メモリを抑えたい場合:

```rust
Record::builder().limit(200).build()
```

---

## 実装ステップ

### Sub-step 7.2.1 — Cargo.toml に `undo` を追加

```toml
undo = "0.52"
```

`cargo check` でビルドが通ることを確認。

---

### Sub-step 7.2.2 — `TextEdit` コマンド型を定義

`src/ui/editor_history.rs` を新規作成:

```rust
use std::time::Instant;
use undo::{Edit, Merge};

pub struct TextEdit {
    pub before: String,
    pub after: String,
    pub timestamp: Instant,
}

impl Edit for TextEdit {
    type Target = String;
    type Output = ();

    fn edit(&mut self, target: &mut String) -> Self::Output {
        *target = self.after.clone();
    }

    fn undo(&mut self, target: &mut String) -> Self::Output {
        *target = self.before.clone();
    }

    fn merge(&mut self, other: &Self) -> Merge {
        // 500ms 以内 かつ 単語/行境界でない → マージ
        if self.timestamp.elapsed().as_millis() < 500
            && !self.after.ends_with([' ', '\n', '(', ')', ':', ','])
        {
            self.after = other.after.clone();
            Merge::Yes
        } else {
            Merge::No
        }
    }
}
```

---

### Sub-step 7.2.3 — `EditorHistory` Resource を追加

`src/ui/components.rs` に追加:

```rust
use undo::Record;
use crate::ui::editor_history::TextEdit;

#[derive(Resource)]
pub struct EditorHistory {
    pub record: Record<String>,
    /// undo/redo による書き換え中は CosmicTextChanged を無視するフラグ
    pub replaying: bool,
}

impl Default for EditorHistory {
    fn default() -> Self {
        Self {
            record: Record::new(),
            replaying: false,
        }
    }
}
```

`UiPlugin::build` に登録:

```rust
app.init_resource::<EditorHistory>();
```

---

### Sub-step 7.2.4 — テキスト変更を Record に push する

`sync_editor_to_strategy_buffer_system` を修正し、変更を履歴に積む。

```rust
pub fn sync_editor_to_strategy_buffer_system(
    mut events: EventReader<CosmicTextChanged>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut buffer: ResMut<StrategyBuffer>,
    mut history: ResMut<EditorHistory>,
) {
    for CosmicTextChanged((entity, new_text)) in events.read() {
        if !editor_q.contains(*entity) { continue; }
        if buffer.source == *new_text { continue; }
        if history.replaying { continue; }  // undo/redo 由来の変更は積まない

        let edit = TextEdit {
            before: buffer.source.clone(),
            after: new_text.clone(),
            timestamp: std::time::Instant::now(),
        };
        // Record::edit は内部で source を書き換えるが、ここでは source をターゲットにしない。
        // source の更新は従来通り手動で行い、Record には "差分オブジェクト" だけ積む。
        // undo 時は record.undo が before を返すので、その値を source に書き戻す。
        history.record.edit(&mut buffer.source, edit);
        buffer.dirty = true;
    }
}
```

> **注意**: `Record::edit` を呼ぶと内部で `edit.edit(&mut target)` = `*target = after.clone()` が実行され  
> `buffer.source` が `new_text` に更新される。`CosmicTextChanged` で届く値と同じなので副作用なし。

---

### Sub-step 7.2.5 — Ctrl+Z / Ctrl+Y system

```rust
pub fn undo_redo_system(
    keys: Res<ButtonInput<KeyCode>>,
    focused: Res<FocusedWidget>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut buffer: ResMut<StrategyBuffer>,
    mut history: ResMut<EditorHistory>,
    mut open_events: EventWriter<OpenStrategyRequested>,
) {
    // エディタにフォーカスがないときは無視
    let Some(focused_entity) = focused.0 else { return; };
    if editor_q.get(focused_entity).is_err() { return; }

    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl { return; }

    let undo_pressed = keys.just_pressed(KeyCode::KeyZ);
    let redo_pressed = keys.just_pressed(KeyCode::KeyY)
        || (keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            && keys.just_pressed(KeyCode::KeyZ));

    if !undo_pressed && !redo_pressed { return; }

    history.replaying = true;

    if undo_pressed {
        history.record.undo(&mut buffer.source);
    } else {
        history.record.redo(&mut buffer.source);
    }

    history.replaying = false;
    buffer.dirty = true;

    // エディタ表示を buffer.source で上書きする
    // sync_strategy_buffer_to_editor_system は OpenStrategyRequested イベントで動くので流用。
    open_events.send(OpenStrategyRequested { path: buffer.original_path.clone().unwrap_or_default() });
}
```

> **代替案**: `OpenStrategyRequested` を使わず、`UndoRedoApplied` という専用 Event を追加して  
> `sync_strategy_buffer_to_editor_system` をそのイベントでも動かす方が意味が明確。  
> 実装時にどちらが綺麗か判断すること。

---

### Sub-step 7.2.6 — system 登録順序

```rust
.add_systems(Update, (
    sync_editor_to_strategy_buffer_system,
    undo_redo_system
        .after(sync_editor_to_strategy_buffer_system),
    sync_strategy_buffer_to_editor_system
        .after(undo_redo_system),
))
```

---

### Sub-step 7.2.7 — ボタン UI (オプション)

タイトルバーに `↩ Undo` / `↪ Redo` ボタンを追加する。  
`spawn_title_bar_button` で `StrategyUndoButton` / `StrategyRedoButton` を追加し、  
`history.record.can_undo()` / `can_redo()` で alpha を制御。

---

## ファイル変更一覧

| ファイル | 変更種別 | 内容 |
|----------|----------|------|
| `Cargo.toml` | 追記 | `undo = "0.52"` |
| `src/ui/editor_history.rs` | 新規 | `TextEdit` コマンド型 |
| `src/ui/components.rs` | 変更 | `EditorHistory` resource |
| `src/ui/strategy_editor.rs` | 変更 | `undo_redo_system`, `sync_editor_to_strategy_buffer_system` 修正 |
| `src/ui/mod.rs` | 変更 | `mod editor_history;` 追加 |

---

## 既知 Caveat

1. **`replaying` フラグの競合**  
   `undo_redo_system` が `buffer.source` を書き換えると `CosmicTextChanged` が発火する可能性がある。  
   `replaying = true` の間は `sync_editor_to_strategy_buffer_system` で push をスキップする。

2. **CosmicEditor の set_text タイミング** (memory #4545)  
   Undo 後のエディタ反映は `CosmicEditBuffer` と `CosmicEditor` 両方に `set_text` が必要。  
   `sync_strategy_buffer_to_editor_system` が両方更新していることを確認する。

3. **ファイル Open 時に履歴をリセット**  
   `open_strategy_buffer_system` の末尾で `history.record = Record::new()` を呼ぶ。  
   別ファイルを開いたとき前のファイルの履歴が残ると混乱の元。

4. **`Instant` は serde 非対応**  
   `TextEdit.timestamp` に `std::time::Instant` を使うと serde 不可。  
   将来セーブ機能が必要になったら `f64` (elapsed seconds) に変える。

---

## 完了条件

- [ ] Ctrl+Z でひとつ前のテキスト状態に戻る
- [ ] Ctrl+Y (または Ctrl+Shift+Z) でやり直しができる
- [ ] 連続入力は 500ms または単語境界でひとつの undo 単位にまとまる
- [ ] 別ファイルを開いたとき履歴がリセットされる
- [ ] `history.record.can_undo() == false` のとき Ctrl+Z は何も起きない
- [ ] `cargo check` / `cargo test` が通る
- [ ] E2E: test_strategy_daily.py を開いて編集 → Ctrl+Z で元に戻ることを目視確認
