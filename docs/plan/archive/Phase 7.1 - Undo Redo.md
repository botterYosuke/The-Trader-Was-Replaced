# Phase 7.1 — Undo / Redo

## Overview

**単一の Undo/Redo タイムライン** を実装し、以下 4 種類のユーザー操作を同じ Ctrl+Z / Ctrl+Y で巻き戻し・やり直しできるようにする:

1. **Strategy Editor のテキスト編集**（`bevy_cosmic_edit 0.26` は Undo/Redo を削除済みのため自前実装）
2. **Floating Window のドラッグ移動**（位置変更）
3. **Floating Window のスポーン**（パネルを開く）
4. **Floating Window のデスポーン**（× ボタンで閉じる）

**履歴は単一タイムライン**で intermix される。すなわち「テキスト編集 → ウィンドウ移動 → テキスト編集」の順に操作した場合、Ctrl+Z 1 回で「最後のテキスト編集」のみが戻り、もう 1 回で「ウィンドウ移動」が戻る ("last action wins")。

採用 crate は引き続き [`undo`](https://crates.io/crates/undo) v0.52。

---

## 現在地 (2026-05-15 時点)

| 項目 | 状態 |
|------|------|
| ブランチ | `feature/7.1-Undo-Redo` (Phase 7.7 完了後に新規作成) |
| Undo/Redo | **未実装** — cosmic_edit 0.26 から削除済み |
| テキスト変更イベント | `CosmicTextChanged(Entity, String)` で全文が届く |
| Floating Window | [src/ui/floating_window.rs](../../src/ui/floating_window.rs) に統一実装。`PanelKind` 6 種、`WindowRoot` マーカー、`TitleBar` の `Pointer<Drag>` で移動、`CloseButton` の `Pointer<Click>` で `despawn_recursive` |
| ウィンドウレイアウトのスナップショット型 | `WindowLayout { kind, visible, position, size, z }` が既に [src/ui/layout_persistence.rs:38](../../src/ui/layout_persistence.rs#L38) に存在 — Phase 7.7 で Save/Load 用に作成済み |

**関連ファイル:**

- [src/ui/strategy_editor.rs](../../src/ui/strategy_editor.rs) — `sync_editor_to_strategy_buffer_system` / `sync_strategy_buffer_to_editor_system`
- [src/ui/components.rs](../../src/ui/components.rs) — `StrategyBuffer`, `PanelKind`, `WindowRoot`, `PanelSpawnRequested`, `CloseButton`
- [src/ui/floating_window.rs](../../src/ui/floating_window.rs) — `panel_spawn_dispatcher_system`, ドラッグ observer, クローズ observer
- [src/ui/layout_persistence.rs](../../src/ui/layout_persistence.rs) — `WindowLayout`, `mark_dirty_on_drag_system`（DragEnd の既存 observer の参考実装）
- [src/ui/menu_bar.rs](../../src/ui/menu_bar.rs) — `open_strategy_buffer_system`
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
| **`undo` v0.52** | 活発メンテ・`merge()` あり・Bevy 非依存・任意の `Target` 型を取れる |
| `undo_2` v0.2.1 | 開発停滞、API が低レベル過ぎる |
| 手書きスナップショット | merge なし・上限管理が面倒 |

### 主要 API

```rust
use undo::{Edit, Record};

let mut record: Record<MyTarget> = Record::builder().limit(200).build();
record.edit(&mut target, MyEdit { /* ... */ });
record.undo(&mut target);
record.redo(&mut target);
```

---

## 設計方針

### Target は ECS ではなく "保留キュー" `PendingAppEdits`

ウィンドウ操作（spawn / despawn / move）の適用には `Commands` や `Query<&mut Transform>` 経由の
ECS ミューテーションが必要だが、`undo::Edit::edit / undo` のシグネチャは
`fn edit(&mut self, target: &mut Target)` で **`&mut World` を取れない**。

そこで **Edit の適用 (ECS mutation) と Edit の記録 (Record 操作) を分離** する:

```text
Edit::edit / Edit::undo
   ↓ push "適用すべき操作" を PendingAppEdits キューに積むだけ
PendingAppEdits (Resource, VecDeque<AppEditAction>)
   ↓ 翌フレーム or 同フレーム後段
apply_pending_app_edits_system   ← ここで Commands / Query を使って ECS に反映
```

- `Record<AppEdit>` を持ち、`impl Edit for AppEdit { type Target = PendingAppEdits; ... }`。
  `undo::Record<E>` の `E` は Edit 型なので、保存される型は `AppEdit` 自身。`PendingAppEdits` は
  `<AppEdit as Edit>::Target` として `record.edit(&mut target, ...)` の第 1 引数に渡る。
- `AppEdit::edit(&mut PendingAppEdits)` は `target.queue.push_back(AppEditAction::Apply...)`。
- `AppEdit::undo(&mut PendingAppEdits)` は逆操作を push。
- 別 system `apply_pending_app_edits_system` が毎フレーム `queue` を drain して
  `Commands::spawn` / `entity.despawn_recursive()` / `Transform.translation = ...` /
  `StrategyBuffer.source = ...` を実行する。

この分離により Edit 実装は純粋関数になり、テストも容易になる
（`PendingAppEdits` の中身を assert すればよい）。

### `AppEdit` enum

```rust
pub enum AppEdit {
    Text(TextEdit),                    // Strategy Editor のテキスト
    WindowMove(WindowMoveEdit),        // ドラッグ移動 1 回分
    WindowSpawn(WindowSpawnEdit),      // パネルを開いた
    WindowDespawn(WindowDespawnEdit),  // × で閉じた
}
```

各 variant の詳細は 7.1.2 で定義する。

### 単一タイムライン vs. パネル別タイムライン

**採用: 単一タイムライン**。

| 案 | 採用理由 / 見送り理由 |
|----|----------------------|
| **単一 `Record<AppEdit>`**（採用） | 「last action wins」で挙動が直感的。VSCode / Figma など多くのツールがこの方式。実装も単純 |
| パネル別 `Record<...>` を複数持つ | 「今フォーカスしているパネルの履歴だけ戻したい」要求があるなら有効だが、ウィンドウ操作はパネル非依存（どこにフォーカスがあっても起きうる）なので破綻する |

代替案として「テキスト履歴」と「ウィンドウ履歴」を別建てにする選択肢もあるが、
ユーザー視点で「Ctrl+Z を押したら直前の操作が戻る」一貫性が失われるので不採用。

### ドラッグの debounce: DragStart→DragEnd で 1 entry

`Pointer<Drag>` は 1 ピクセル動くごとに発火するため、そのまま push すると履歴が
1 ドラッグで数十〜数百件になる。**`Pointer<DragStart>` で `before_pos` を記録し、
`Pointer<DragEnd>` で `after_pos` を確定して 1 件だけ push** する。

- `ActiveDrag(HashMap<Entity, Vec2>)` Resource で DragStart 時の `Transform.translation.xy()` を保持。
- DragEnd 時に現在位置と比較し、変化がなければ push しない（クリック扱い）。

> **代替案**: 「連続するウィンドウ移動を時間でマージ」する案もあるが、テキストと違い
> ウィンドウ操作はユーザーが意図して 1 ドラッグ＝ 1 単位と認識するので、マージ不要。

### Merge ポリシー（操作種別ごと）

| Edit 種別 | Merge 戦略 |
|-----------|-----------|
| `Text` | 500ms 以内 & 末尾が単語境界文字でない & paste/改行追加でない → `Merged::Yes`。それ以外は `Merged::No(other)` |
| `WindowMove` | **マージしない**（`Merged::No(other)` 固定）。1 ドラッグ＝ 1 件で既に粒度が適切 |
| `WindowSpawn` | マージしない (`Merged::No(other)`) |
| `WindowDespawn` | マージしない (`Merged::No(other)`) |

異種 Edit のマージは undo crate の仕様上、隣接する **同じ `Edit` 型** 同士でしか
試行されないので問題ない（enum で包んでいる場合は `merge` を `match` で実装）。

### 履歴上限

`Record::builder().limit(200).build()` — テキスト・ウィンドウ操作を区別せず合計 200 件。

---

## 実装ステップ

### Sub-step 7.1.0 — 現状コード確認 (verified ✅)

着手前に [src/ui/strategy_editor.rs](../../src/ui/strategy_editor.rs) と
[src/ui/floating_window.rs](../../src/ui/floating_window.rs) を再読し、以下を確認した。

#### Strategy Editor の set_text パス

[src/ui/strategy_editor.rs:265-305](../../src/ui/strategy_editor.rs#L265) を確認した結果、
**`CosmicEditBuffer` と `CosmicEditor` 内部 Buffer の両方に `set_text` を呼んでいる**。
これは memory `cosmic-edit Buffer メトリクスの DPI トラップ` で記録した
「`CosmicEditBuffer` は DPI 倍されるが `CosmicEditor` 内部 buffer は倍されない」
問題を踏まえた実装で、Phase 7.1 でそのまま流用できる。状態: **verified ✅**。

```rust
// [src/ui/strategy_editor.rs:288](../../src/ui/strategy_editor.rs#L288)
edit_buffer.set_text(&mut font_system, &buffer.source, Attrs::new());

// [src/ui/strategy_editor.rs:293-303](../../src/ui/strategy_editor.rs#L293)
if let Some(mut editor) = editor_opt {
    editor.with_buffer_mut(|b| {
        b.set_text(&mut font_system, &buffer.source, Attrs::new(), Shaping::Advanced);
        b.set_redraw(true);
    });
}
```

ただしこの system は現在 `OpenStrategyRequested` でのみトリガする
（[src/ui/strategy_editor.rs:266](../../src/ui/strategy_editor.rs#L266)）。
Undo/Redo 後にも同じ set_text パスを発火させたいので、7.1.5 で
`UndoRedoApplied` event を追加し、本 system を両方の event で動かす。

#### `buffer.source` の更新箇所

`buffer.source` を書き換える system は現状以下のみ:

- [src/ui/strategy_editor.rs:330](../../src/ui/strategy_editor.rs#L330) `sync_editor_to_strategy_buffer_system`
- [src/ui/menu_bar.rs:219](../../src/ui/menu_bar.rs#L219) `open_strategy_buffer_system`

`CosmicTextChanged` イベントが到着した瞬間、`buffer.source` には
**まだ旧値が入っている**ことが保証できる。7.1.4 の `before = buffer.source.clone()` はこの順序前提に依存。

#### Floating Window 周辺の挙動 (verified ✅)

[src/ui/floating_window.rs](../../src/ui/floating_window.rs) を読み、以下を確認した。

**スポーン**: 一元化された dispatcher 経由。

```rust
// [src/ui/floating_window.rs:182-205](../../src/ui/floating_window.rs#L182)
pub fn panel_spawn_dispatcher_system(
    mut events: EventReader<PanelSpawnRequested>,
    existing: Query<&PanelKind, With<WindowRoot>>,
    ...
) {
    for event in events.read() {
        if existing.iter().any(|k| *k == event.kind) { continue; }
        match event.kind { /* PanelKind ごとに spawn 関数を呼ぶ */ }
    }
}
```

`PanelSpawnRequested { kind: PanelKind }` を発火するのは:

- [src/ui/sidebar.rs:218](../../src/ui/sidebar.rs#L218), [:224](../../src/ui/sidebar.rs#L224) — サイドバーのボタン操作（**ユーザー操作**）
- [src/ui/layout_persistence.rs:273](../../src/ui/layout_persistence.rs#L273) — Layout Load 時の自動 spawn（**履歴に積みたくない**）

undo で再 spawn する際もこのイベントを再発火する形にする（重複防止ロジックを流用できるし、
将来 spawn 時の初期化ロジックが増えても 1 か所に集約できる）。

**発生源の区別が必要 (Findings 3)**: dispatcher 側で「ユーザー操作の spawn は history に push、
Layout Load / Undo/Redo 由来の spawn は push しない」を判定するため、event を以下のように拡張する:

```rust
// src/ui/components.rs
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSpawnRequested {
    pub kind: PanelKind,
    pub source: PanelSpawnSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSpawnSource {
    /// サイドバーのボタン / メニューなどユーザー操作由来
    User,
    /// `apply_layout_system` 経由の sidecar Load
    LayoutLoad,
    /// `apply_pending_app_edits_system` 経由の undo/redo 再 spawn
    UndoRedo,
}
```

呼び出し元の更新が必要なファイル:

| 呼出元 | 設定する source |
|--------|----------------|
| `src/ui/sidebar.rs:218, :224` | `PanelSpawnSource::User` |
| `src/ui/layout_persistence.rs:273` | `PanelSpawnSource::LayoutLoad` |
| `apply_pending_app_edits_system`（新規） | `PanelSpawnSource::UndoRedo` |

`panel_spawn_dispatcher_system` 側では「`source == User` の場合のみ history.push を呼ぶ」分岐を追加する。

**ドラッグ移動**: タイトルバーの `Pointer<Drag>` observer。

```rust
// [src/ui/floating_window.rs:96-110](../../src/ui/floating_window.rs#L96)
.observe(|drag: Trigger<Pointer<Drag>>,
          mut query: Query<&mut Transform, With<WindowRoot>>,
          parent_query: Query<&Parent>,
          camera_query: Query<&OrthographicProjection, With<Camera2d>>| {
    if let Ok(parent) = parent_query.get(drag.entity()) {
        if let Ok(mut transform) = query.get_mut(parent.get()) {
            let scale = camera_query.get_single().map(|p| p.scale).unwrap_or(1.0);
            transform.translation.x += drag.event().delta.x * scale;
            transform.translation.y -= drag.event().delta.y * scale;
        }
    }
});
```

**重要**: 現状 **`Pointer<DragStart>` や `Pointer<DragEnd>` の hook は存在しない**
（layout_persistence.rs:379 の `mark_dirty_on_drag_system` は `DragEnd` だけを
グローバル observer で見ているが、これは Auto-Save 用で `before_pos` は記録していない）。
よって 7.1 で **新規に DragStart observer と DragEnd observer を追加** する必要がある。

**デスポーン**: × ボタンの `Pointer<Click>` observer。

```rust
// [src/ui/floating_window.rs:150-159](../../src/ui/floating_window.rs#L150)
.observe(|trigger: Trigger<Pointer<Click>>,
          parent_query: Query<&Parent>,
          mut commands: Commands| {
    if let Ok(parent) = parent_query.get(trigger.entity()) {
        commands.entity(parent.get()).despawn_recursive();
    }
});
```

**`Visibility::Hidden` ではなく実 ECS 削除**。よって undo で復活させるには
`PanelSpawnRequested` を発火して再 spawn し、保存済みの位置・サイズ・z を当て直す必要がある。
Strategy Editor の場合は加えて `StrategyBuffer.source` の内容も復元する。

`WindowLayout` 型（[src/ui/layout_persistence.rs:38](../../src/ui/layout_persistence.rs#L38)）が
`{ kind, visible, position, size, z }` をすでに持っているので、
`WindowSpawnEdit` / `WindowDespawnEdit` の "before/after snapshot" の格納にそのまま流用する。

### Sub-step 7.1.0b — キーバインド衝突確認 (verified ✅)

`src/` 配下の `KeyCode::KeyZ` / `KeyCode::KeyY` / `KeyCode::ControlLeft|ControlRight` を全件確認:

| 場所 | 内容 | 衝突可能性 |
|------|------|-----------|
| [src/camera.rs:41](../../src/camera.rs#L41) | `ctrl = any_pressed([ControlLeft, ControlRight])` をズーム/パン抑制判定に使用。`KeyZ`/`KeyY` は触らない | なし |
| [src/ui/layout_persistence.rs:487](../../src/ui/layout_persistence.rs#L487) | Ctrl+S / Ctrl+Shift+S / Ctrl+O のみ。`KeyZ`/`KeyY` は触らない | なし |

`KeyCode::KeyZ` / `KeyCode::KeyY` を `just_pressed` で消費している system は現状ゼロ。
そのまま `undo_redo_system` に割り当てて衝突は起きない。

**採用キーバインド** (OQ1 対応 — Windows / JIS 配列も考慮し 3 つすべて正式対応):

| 操作 | キー |
|------|------|
| Undo | `Ctrl + Z` |
| Redo | `Ctrl + Y` |
| Redo (Mac/VSCode 互換) | `Ctrl + Shift + Z` |

**フォーカスガードの方針変更**: 旧プランは「エディタにフォーカスがあるときのみ undo」だったが、
Phase 7.1 の新スコープでは **ウィンドウ移動・spawn・despawn もエディタ非フォーカスで発生する**
ため、Ctrl+Z はグローバルに有効化する（7.1.5 参照）。

---

### Sub-step 7.1.1 — Cargo.toml に `undo` を追加

```toml
undo = "0.52"
```

`cargo check` でビルドが通ることを確認。

---

### Sub-step 7.1.2 — `AppEdit` enum とサブ Edit 型を定義

`src/ui/editor_history.rs` を新規作成:

```rust
use bevy::prelude::Entity;
use std::collections::VecDeque;
use std::time::Instant;
use undo::{Edit, Merged};

use crate::ui::components::PanelKind;
use crate::ui::layout_persistence::WindowLayout;

/// Edit の適用（ECS への反映）を保留しておくキュー。`undo::Record` の Target。
#[derive(Default)]
pub struct PendingAppEdits {
    pub queue: VecDeque<AppEditAction>,
}

/// 別 system が drain して ECS を実際に書き換えるためのアクション。
pub enum AppEditAction {
    SetStrategySource(String),                 // StrategyBuffer.source ← new value
    MoveWindow { kind: PanelKind, to: [f32; 2] },
    /// 再 spawn 操作。Strategy Editor の場合は未保存テキストも復元したいので snapshot を持つ。
    SpawnWindow {
        layout: WindowLayout,
        strategy_snapshot: Option<StrategySnapshot>,
    },
    DespawnWindow { kind: PanelKind },
}

/// 全 Edit 種別を包む enum。
pub enum AppEdit {
    Text(TextEdit),
    WindowMove(WindowMoveEdit),
    WindowSpawn(WindowSpawnEdit),
    WindowDespawn(WindowDespawnEdit),
}

// ── テキスト編集 ──────────────────────────────
pub struct TextEdit {
    pub before: String,
    pub after: String,
    pub timestamp: Instant,
}

// ── ウィンドウ移動 ────────────────────────────
pub struct WindowMoveEdit {
    pub kind: PanelKind,
    pub before_pos: [f32; 2],
    pub after_pos: [f32; 2],
}

// ── ウィンドウ spawn ──────────────────────────
/// パネルを「開く」操作。undo すると despawn される。
pub struct WindowSpawnEdit {
    pub layout: WindowLayout,
    /// Strategy Editor の場合のみ、spawn 時点の buffer.source と original_path を保持
    /// （undo→redo で再 spawn したときにテキストを復元するため）。
    pub strategy_snapshot: Option<StrategySnapshot>,
}

// ── ウィンドウ despawn ────────────────────────
/// パネルを「閉じる」操作。undo すると再 spawn される。
pub struct WindowDespawnEdit {
    pub layout: WindowLayout,
    pub strategy_snapshot: Option<StrategySnapshot>,
}

#[derive(Clone)]
pub struct StrategySnapshot {
    pub source: String,
    pub original_path: Option<std::path::PathBuf>,
}

impl Edit for AppEdit {
    type Target = PendingAppEdits;
    type Output = ();

    fn edit(&mut self, target: &mut PendingAppEdits) {
        match self {
            AppEdit::Text(e) => target.queue.push_back(
                AppEditAction::SetStrategySource(e.after.clone())),
            AppEdit::WindowMove(e) => target.queue.push_back(
                AppEditAction::MoveWindow { kind: e.kind, to: e.after_pos }),
            AppEdit::WindowSpawn(e) => target.queue.push_back(
                AppEditAction::SpawnWindow {
                    layout: e.layout.clone(),
                    strategy_snapshot: e.strategy_snapshot.clone(),
                }),
            AppEdit::WindowDespawn(e) => target.queue.push_back(
                AppEditAction::DespawnWindow { kind: e.layout.kind }),
        }
    }

    fn undo(&mut self, target: &mut PendingAppEdits) {
        match self {
            AppEdit::Text(e) => target.queue.push_back(
                AppEditAction::SetStrategySource(e.before.clone())),
            AppEdit::WindowMove(e) => target.queue.push_back(
                AppEditAction::MoveWindow { kind: e.kind, to: e.before_pos }),
            AppEdit::WindowSpawn(e) => target.queue.push_back(
                AppEditAction::DespawnWindow { kind: e.layout.kind }),
            AppEdit::WindowDespawn(e) => target.queue.push_back(
                AppEditAction::SpawnWindow {
                    layout: e.layout.clone(),
                    strategy_snapshot: e.strategy_snapshot.clone(),
                }),
        }
    }

    // undo 0.52 の `merge` シグネチャは `fn merge(&mut self, other: Self) -> Merged<Self>`。
    // 未マージのときは `Merged::No(other)` で他方を返す（呼び出し側がそれを stack に積み直す）。
    fn merge(&mut self, other: Self) -> Merged<Self>
    where
        Self: Sized,
    {
        // テキスト同士のみマージ。ウィンドウ操作はすべて Merged::No(other)。
        // self は変更しうるが、other は consume するか返すかの二択。
        match (&mut *self, &other) {
            (AppEdit::Text(a), AppEdit::Text(b)) => {
                let mergeable = a.timestamp.elapsed().as_millis() < 500
                    && !a.after.ends_with([' ', '\n', '(', ')', ':', ','])
                    // paste / multi-line 一括編集はマージしない（Caveat 10 参照）
                    && b.after.len().saturating_sub(a.after.len()) <= 50
                    && !b.after[a.after.len().min(b.after.len())..].contains('\n');
                if mergeable {
                    a.after = b.after.clone();
                    a.timestamp = b.timestamp;
                    Merged::Yes
                } else {
                    Merged::No(other)
                }
            }
            _ => Merged::No(other),
        }
    }
}
```

> **設計判断**: `SpawnWindow` アクションが `WindowLayout` を丸ごと運ぶことで、
> 「Strategy Editor を閉じる→ undo」のときに保存しておいた位置・サイズ・z をそのまま
> 復元できる。`PanelSpawnRequested` を投げた後、続けて `WindowLayout` の position/size/z
> を `apply_pending_layout_system` 経由で当て直す方式
> （[layout_persistence.rs:317](../../src/ui/layout_persistence.rs#L317) の既存ロジックを再利用）。
>
> `strategy_snapshot` を最初から variant に含めるのが Findings 4 への対応。
> 未保存編集中の Strategy Editor を閉じて Undo した場合、disk から読み直すと未保存分が
> 失われるため、close 時の `buffer.source` を snapshot として運ぶ必要がある。

---

### Sub-step 7.1.3 — Resources / Events

**配置方針 (Findings 5)**: `AppHistory` / `PendingAppEdits` / `PendingStrategySnapshotRestore` /
`UndoRedoApplied` は **すべて `editor_history.rs` 側に置く**。`Record<AppEdit>` が
`AppEdit` 型に依存するため、components.rs に逆参照させると循環依存になりやすい。
components.rs は純 UI component / event（`PanelKind`, `WindowRoot`, `PanelSpawnRequested`,
`MenuTopLevel` 等）に限定。

`src/ui/editor_history.rs` 末尾に以下を追加:

```rust
use bevy::prelude::*;
use undo::Record;

#[derive(Resource)]
pub struct AppHistory {
    pub record: Record<AppEdit>,
    pub pending: PendingAppEdits,
    /// undo/redo 適用中であることを示すカウンタ。
    /// `> 0` の間は「ユーザー操作」由来でない変更とみなし、history への再 push を抑止する。
    /// `undo_redo_system` が `record.undo/redo` 直前に `+= 1` し、
    /// **`apply_pending_app_edits_system` が queue を空に drain した直後に** `-= 1` する。
    /// `bool` でなくカウンタにする理由は、undo 中に発生する CosmicTextChanged や
    /// PanelSpawnRequested が後段フレームで処理されるケースを 1 フレームのスコープでは
    /// 守りきれないため（[Findings 2 / Caveat 1] 参照）。
    pub replaying_depth: u32,
}

impl AppHistory {
    pub fn is_replaying(&self) -> bool { self.replaying_depth > 0 }
}

impl Default for AppHistory {
    fn default() -> Self {
        Self {
            record: Record::builder().limit(200).build(),
            pending: PendingAppEdits::default(),
            replaying_depth: 0,
        }
    }
}

/// ドラッグ移動の before_pos を DragStart で記録するためのリソース。
#[derive(Resource, Default)]
pub struct ActiveDrag {
    pub starts: bevy::utils::HashMap<Entity, Vec2>,
}

/// SpawnWindow の strategy_snapshot を、エディタ entity 生成後まで保持するキュー (Findings 4)。
#[derive(Resource, Default)]
pub struct PendingStrategySnapshotRestore {
    pub queue: Vec<StrategySnapshot>,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct UndoRedoApplied;  // テキスト set_text を発火するため
```

`UiPlugin::build`:

```rust
app.init_resource::<AppHistory>()
   .init_resource::<ActiveDrag>()
   .init_resource::<PendingStrategySnapshotRestore>()
   .add_event::<UndoRedoApplied>();
```

> **設計判断**: 旧プランの `EditorHistory` は `Record<String>` だったので
> `AppHistory` にリネーム + 内部型変更。`pending` も同 Resource にまとめると
> `&mut AppHistory` 1 つで Record と queue を同時に触れて lifetime 問題が起きない。

---

### Sub-step 7.1.4 — 各 push 経路

#### (a) テキスト編集 — `sync_editor_to_strategy_buffer_system`

```rust
pub fn sync_editor_to_strategy_buffer_system(
    mut events: EventReader<CosmicTextChanged>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut buffer: ResMut<StrategyBuffer>,
    mut history: ResMut<AppHistory>,
) {
    for CosmicTextChanged((entity, new_text)) in events.read() {
        if !editor_q.contains(*entity) { continue; }
        if buffer.source == *new_text { continue; }
        if history.is_replaying() { continue; }

        let edit = AppEdit::Text(TextEdit {
            before: buffer.source.clone(),
            after: new_text.clone(),
            timestamp: std::time::Instant::now(),
        });
        // Record::edit は edit() を呼んで pending に push するだけ。
        // buffer.source への実書き込みは apply_pending_app_edits_system が行う。
        let AppHistory { record, pending, .. } = &mut *history;
        record.edit(pending, edit);
        buffer.dirty = true;
    }
}
```

> **重要な順序前提**: `CosmicTextChanged` が届いた瞬間 `buffer.source` には旧値、
> `new_text` には新値が入っている。`record.edit` を呼ぶと `pending.queue` に
> `SetStrategySource(new_text)` が積まれるが、これは次フレームに
> `apply_pending_app_edits_system` で `buffer.source` に書かれる。
> このタイムラグの間に他 system が `buffer.source` を読むと旧値を見る。
> Phase 7.1 時点では `buffer.source` を読む system は描画再同期だけなので問題ないが、
> 将来「同フレーム内で `buffer.source` 経由で動く system」を追加するときは
> `apply_pending_app_edits_system` を **先に** 走らせる依存を張ること。

#### (b) ウィンドウ移動 — DragStart / DragEnd observer

`src/ui/floating_window.rs` の `spawn_floating_window` 内、TitleBar の `.observe(...)` を拡張する。
既存の `Pointer<Drag>` observer は残し（リアルタイム追従に必須）、新たに 2 つ追加:

```rust
.observe(|trigger: Trigger<Pointer<DragStart>>,
          parent_query: Query<&Parent>,
          tf_q: Query<&Transform, With<WindowRoot>>,
          mut active: ResMut<ActiveDrag>| {
    if let Ok(parent) = parent_query.get(trigger.entity()) {
        if let Ok(tf) = tf_q.get(parent.get()) {
            active.starts.insert(parent.get(), tf.translation.truncate());
        }
    }
})
.observe(|trigger: Trigger<Pointer<DragEnd>>,
          parent_query: Query<&Parent>,
          tf_q: Query<(&Transform, &PanelKind), With<WindowRoot>>,
          mut active: ResMut<ActiveDrag>,
          mut history: ResMut<AppHistory>| {
    let Ok(parent) = parent_query.get(trigger.entity()) else { return; };
    let parent_entity = parent.get();
    let Some(before) = active.starts.remove(&parent_entity) else { return; };
    let Ok((tf, kind)) = tf_q.get(parent_entity) else { return; };
    let after = tf.translation.truncate();
    if (after - before).length_squared() < 0.5 { return; } // クリック相当

    let edit = AppEdit::WindowMove(WindowMoveEdit {
        kind: *kind,
        before_pos: before.to_array(),
        after_pos: after.to_array(),
    });
    if history.is_replaying() { return; }
    let AppHistory { record, pending, .. } = &mut *history;
    record.edit(pending, edit);
});
```

> **注**: ここで `record.edit` を呼んでも `pending.queue` に
> `MoveWindow { to: after }` が積まれるだけで、Transform はすでにドラッグ中に
> 更新済み。redo 時のために push は必要。

#### (c) ウィンドウ spawn — `panel_spawn_dispatcher_system` 末尾

```rust
// 既存 dispatcher の各 arm で spawn 関数を呼んだ直後に追加。
// spawn 関数が (root, content, title_bar) を返すよう統一済みなら root から
// 初期位置・サイズ・kind を取れる（あるいは spawn 関数自身に history.push を組み込む）。
fn record_window_spawn(
    history: &mut AppHistory,
    kind: PanelKind,
    initial_pos: Vec2,
    initial_size: Vec2,
    z: f32,
    strategy_snapshot: Option<StrategySnapshot>,
) {
    if history.is_replaying() { return; }
    let edit = AppEdit::WindowSpawn(WindowSpawnEdit {
        layout: WindowLayout {
            kind,
            visible: true,
            position: initial_pos.to_array(),
            size: initial_size.to_array(),
            z,
        },
        strategy_snapshot,
    });
    let AppHistory { record, pending, .. } = &mut *history;
    record.edit(pending, edit);
}
```

呼び出し位置: `panel_spawn_dispatcher_system` の各 `match` arm の直後。
ただし **以下 2 ケースでは push してはいけない**:

1. `apply_pending_app_edits_system` 経由の再 spawn（= undo/redo）
2. `apply_layout_system` 経由の sidecar Load

これらは `event.source` で判別する:

```rust
if event.source == PanelSpawnSource::User && !history.is_replaying() {
    record_window_spawn(&mut history, kind, initial_pos, initial_size, z, snapshot);
}
```

`history.is_replaying()` も二重の保険として残す（source の渡し忘れに気づきやすくなる）。

#### (d) ウィンドウ despawn — `CloseButton` の Click observer

[src/ui/floating_window.rs:150-159](../../src/ui/floating_window.rs#L150) の observer を拡張:

```rust
.observe(|trigger: Trigger<Pointer<Click>>,
          parent_query: Query<&Parent>,
          root_q: Query<(&PanelKind, &Transform, &Sprite), With<WindowRoot>>,
          buffer: Res<StrategyBuffer>,
          mut history: ResMut<AppHistory>,
          mut commands: Commands| {
    let Ok(parent) = parent_query.get(trigger.entity()) else { return; };
    let parent_entity = parent.get();
    if let Ok((kind, tf, sprite)) = root_q.get(parent_entity) {
        let snapshot = if *kind == PanelKind::StrategyEditor {
            Some(StrategySnapshot {
                source: buffer.source.clone(),
                original_path: buffer.original_path.clone(),
            })
        } else { None };
        if !history.is_replaying() {
            let layout = WindowLayout {
                kind: *kind,
                visible: true,
                position: [tf.translation.x, tf.translation.y],
                size: sprite.custom_size.unwrap_or(Vec2::ZERO).to_array(),
                z: tf.translation.z,
            };
            let AppHistory { record, pending, .. } = &mut *history;
            record.edit(pending, AppEdit::WindowDespawn(
                WindowDespawnEdit { layout, strategy_snapshot: snapshot }));
        }
    }
    commands.entity(parent_entity).despawn_recursive();
});
```

---

### Sub-step 7.1.5 — Ctrl+Z / Ctrl+Y system（グローバル）

```rust
pub fn undo_redo_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut history: ResMut<AppHistory>,
    mut applied_w: EventWriter<UndoRedoApplied>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl { return; }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let undo_pressed = !shift && keys.just_pressed(KeyCode::KeyZ);
    let redo_pressed = keys.just_pressed(KeyCode::KeyY)
        || (shift && keys.just_pressed(KeyCode::KeyZ));
    if !undo_pressed && !redo_pressed { return; }

    // depth を上げてから record.undo/redo を呼ぶ。`record.undo` の中で発火する
    // AppEdit::edit/undo (= pending への push) は this frame で完結するが、その後の
    // apply_pending_app_edits_system / sync_strategy_buffer_to_editor_system /
    // panel_spawn_dispatcher_system / Drag observer が翌フレーム以降に処理する
    // 中間イベント (CosmicTextChanged, Pointer<DragEnd>, PanelSpawnRequested) を
    // ガードする必要があるため、depth はここでは下げない。
    // depth は apply_pending_app_edits_system が queue を空に drain した直後に下げる。
    history.replaying_depth = history.replaying_depth.saturating_add(1);
    let AppHistory { record, pending, .. } = &mut *history;
    let changed = if undo_pressed {
        record.undo(pending).is_some()
    } else {
        record.redo(pending).is_some()
    };

    if changed {
        applied_w.send(UndoRedoApplied);
    } else {
        // pending に何も積まれなかった (record が empty 等) のでガード解除。
        history.replaying_depth = history.replaying_depth.saturating_sub(1);
    }
}
```

**フォーカスガードを撤廃した理由**: ウィンドウ移動・spawn・despawn は
エディタ非フォーカスでも起きうるので、Ctrl+Z はグローバルに有効化する必要がある。
副作用として「テキスト入力欄を持つ別 UI（将来追加されたら）」で Ctrl+Z が
本 system に吸われる可能性があるが、現状そのような UI は無いので問題ない。
追加されたタイミングで再検討する。

> **代替案**: 「エディタにフォーカスがある & 直近の Edit が Text だったときのみ Text を undo、
> それ以外は最新の Edit を undo」のような分岐は実装可能だが、ユーザーの直感
> （"Ctrl+Z は直前の操作を戻す"）と乖離するので採用しない。

---

### Sub-step 7.1.6 — `apply_pending_app_edits_system`

`pending.queue` を drain し、ECS を実際に書き換える。

```rust
pub fn apply_pending_app_edits_system(
    mut history: ResMut<AppHistory>,
    mut buffer: ResMut<StrategyBuffer>,
    mut panels: Query<(Entity, &PanelKind, &mut Transform, &mut Sprite), With<WindowRoot>>,
    mut spawn_w: EventWriter<PanelSpawnRequested>,
    mut pending_layout: ResMut<PendingLayoutApply>,
    mut pending_restore: ResMut<PendingStrategySnapshotRestore>,  // ← 新規 (Findings 4)
    mut applied_w: EventWriter<UndoRedoApplied>,
    mut commands: Commands,
) {
    let mut any_text = false;
    let actions: Vec<_> = history.pending.queue.drain(..).collect();
    for action in actions {
        match action {
            AppEditAction::SetStrategySource(s) => {
                if buffer.source != s {
                    buffer.source = s;
                    buffer.dirty = true;
                    any_text = true;
                }
            }
            AppEditAction::MoveWindow { kind, to } => {
                if let Some((_, _, mut tf, _)) = panels.iter_mut().find(|(_, k, ..)| **k == kind) {
                    tf.translation.x = to[0];
                    tf.translation.y = to[1];
                }
            }
            AppEditAction::SpawnWindow { layout, strategy_snapshot } => {
                // すでに存在する場合は dispatcher が skip するので二重 spawn しない。
                // source = UndoRedo にして panel_spawn_dispatcher_system 側で
                // history への再 push を抑止する（Findings 3 参照）。
                spawn_w.send(PanelSpawnRequested {
                    kind: layout.kind,
                    source: PanelSpawnSource::UndoRedo,
                });
                // 翌フレームに位置・サイズ・z を当て直す既存パイプを再利用
                pending_layout.windows.push(layout.clone());

                // ★ Strategy Editor の内容復元は **2 段階遅延** が必要 (Findings 4)。
                //   spawn_w.send したパネルは dispatcher が次の Update で初めて ECS に
                //   実体を作る。同フレームで buffer.source を書き戻して UndoRedoApplied を
                //   送っても、StrategyEditorContent entity がまだ存在せず set_text が空振りする。
                //   そこで PendingStrategySnapshotRestore に積み、entity 生成後の system が
                //   buffer 書き戻し + UndoRedoApplied 発火を担当する。
                //   sidebar.rs:237 の PendingStrategyLoad と同じパターン。
                if layout.kind == PanelKind::StrategyEditor {
                    if let Some(snap) = strategy_snapshot {
                        pending_restore.queue.push(snap);
                    }
                }
            }
            AppEditAction::DespawnWindow { kind } => {
                if let Some((entity, ..)) = panels.iter().find(|(_, k, ..)| **k == kind) {
                    commands.entity(entity).despawn_recursive();
                }
            }
        }
    }
    // ★ UndoRedoApplied は **replaying 中のテキスト書き戻し** のときだけ送る (Findings 2)。
    //   通常のユーザー入力で SetStrategySource が drain されたケースでも buffer.source は
    //   書き換わるが、その時点で editor 側は既に新しいテキストを表示しているので
    //   set_text を呼ぶとカーソルがリセットされる。replaying 中だけ editor 再同期を強制する。
    if any_text && history.is_replaying() {
        applied_w.send(UndoRedoApplied);
    }

    // depth を queue 完全 drain 後に下げる。中間イベント（CosmicTextChanged など）が
    // この後 1 フレーム内に来る可能性は残るが、Bevy の system 順序を守れば
    // sync_editor_to_strategy_buffer_system はこの system より後段で動くので
    // is_replaying() が false に戻る前に CosmicTextChanged は読まれない。
    // ただし Drag observer は同フレーム内に発火しうるので、保険として
    // 「次フレーム冒頭で 0 に戻す」案も検討（Caveat 1 参照）。
    if history.replaying_depth > 0 {
        history.replaying_depth -= 1;
    }
}
```

> **strategy_snapshot は最初から variant に含む**: `AppEditAction::SpawnWindow` は
> `{ layout, strategy_snapshot: Option<StrategySnapshot> }` 形式（7.1.2 で定義済み）。
> `kind == StrategyEditor && Some(snap)` のとき snapshot を `PendingStrategySnapshotRestore`
> キューに積み、エディタ entity が次フレームで生成された後に新規 system
> `apply_strategy_snapshot_restore_system` が `buffer.source` 書き戻し + `UndoRedoApplied`
> 発火を行う（Findings 4 / Caveat 6 参照）。

#### Snapshot restore system (新規)

```rust
/// 未スポーンの StrategyEditor に対する restore を遅延適用するキュー。
#[derive(Resource, Default)]
pub struct PendingStrategySnapshotRestore {
    pub queue: Vec<StrategySnapshot>,
}

/// `apply_pending_app_edits_system` が積んだ snapshot を、StrategyEditor entity が
/// 生成された後に消化する。sidebar.rs:237 の PendingStrategyLoad と同じパターン。
pub fn apply_strategy_snapshot_restore_system(
    mut pending: ResMut<PendingStrategySnapshotRestore>,
    mut buffer: ResMut<StrategyBuffer>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut applied_w: EventWriter<UndoRedoApplied>,
) {
    if pending.queue.is_empty() { return; }
    // 待っている restore があっても StrategyEditor entity が生成されるまで保持。
    if editor_q.is_empty() { return; }
    for snap in pending.queue.drain(..) {
        buffer.source = snap.source;
        buffer.original_path = snap.original_path;
        buffer.dirty = true;
    }
    applied_w.send(UndoRedoApplied);
}
```

#### system 登録順序

```rust
.add_systems(Update, (
    // 1. ユーザー操作を Record に push
    sync_editor_to_strategy_buffer_system,
    // 2. Ctrl+Z / Y を処理（Record::undo/redo → pending.queue へ push）
    undo_redo_system.after(sync_editor_to_strategy_buffer_system),
    // 3. pending.queue を drain して ECS に反映
    apply_pending_app_edits_system.after(undo_redo_system),
    // 3.5. spawn 後フレームで snapshot を流し込む（Findings 4）
    apply_strategy_snapshot_restore_system.after(apply_pending_app_edits_system),
    // 4. テキスト書き換えをエディタへ反映（UndoRedoApplied で発火）
    sync_strategy_buffer_to_editor_system
        .after(apply_pending_app_edits_system)
        .after(apply_strategy_snapshot_restore_system)
        .after(open_strategy_buffer_system),
    // 5. spawn 要求は dispatcher が翌フレームで拾う（既存）
))
```

**履歴リセットのトリガー条件 (Findings 3 対応)**:

サイドバーから Strategy Editor パネルを開くフローは
`PanelSpawnRequested(StrategyEditor)` → `PendingStrategyLoad` → `OpenStrategyRequested` の連鎖になっており、
直前の `WindowSpawn(StrategyEditor)` push の直後に `open_strategy_buffer_system` が走る。
ここで無条件に履歴をリセットすると、**たった今 push した `WindowSpawn` まで消えて
「Ctrl+Z で開いたばかりのパネルが閉じる」が成立しなくなる**。

対応: リセット条件を **「ロードするファイルパスが現在の `buffer.original_path` と異なるとき」** に限定する。

```rust
// open_strategy_buffer_system 末尾
let path_changed = buffer.original_path.as_ref() != Some(&loaded_path);
if path_changed {
    history.record = Record::builder().limit(200).build();
    history.pending.queue.clear();
    // ActiveDrag も併せて空に
}
```

これにより:

- **新規パネルを開く（既存と同じ最後に閉じたファイルが既定 or path 未設定）**: リセットされず、`WindowSpawn` 履歴は残る → Ctrl+Z でパネルを閉じれる
- **別ファイルを Open Strategy... で開く**: 履歴リセット → 前ファイルの編集履歴は捨てられる
- **同じファイルを再 Open**: リセット不要（履歴を保持してもユーザーは違和感を持たない）

なお Caveat 3 のタイトルは「別ファイルを開いたとき履歴をリセット」の意味に明確化する。

---

### Sub-step 7.1.7 — テスト

#### 単体テスト — `src/ui/editor_history.rs` 末尾

| テスト名 | 内容 |
|----------|------|
| `merge_text_within_500ms_non_boundary` | `AppEdit::Text` 同士 + 500ms 以内 & 末尾非境界 → `Merged::Yes` |
| `merge_text_over_500ms_returns_no` | timestamp ずらして `Merged::No(other)` |
| `merge_blocked_by_space_newline_punct` | 末尾 `' ' '\n' '(' ')' ':' ','` で `Merged::No(other)` |
| `merge_blocked_by_paste_size` | 差分が 50 文字超 → `Merged::No(other)` (paste 想定) |
| `merge_blocked_by_added_newline` | 差分に `\n` 追加 → `Merged::No(other)` |
| `merge_window_move_returns_no` | `WindowMove` 同士でも `Merged::No(other)` 固定 |
| `merge_text_vs_window_returns_no` | 異種の組み合わせは常に `Merged::No(other)` |
| `text_edit_pushes_set_strategy_source` | `record.edit` 後 `pending.queue.back()` が `SetStrategySource(after)` |
| `text_edit_undo_pushes_before_value` | `record.undo` 後 `SetStrategySource(before)` |
| `window_move_edit_round_trip` | `record.edit` → `record.undo` で `MoveWindow` が逆方向に積まれる |
| `window_spawn_undo_pushes_despawn` | `WindowSpawn` の `undo` で `DespawnWindow` が pending に入る |
| `window_despawn_undo_pushes_spawn` | `WindowDespawn` の `undo` で `SpawnWindow(layout)` が pending に入る |
| `mixed_timeline_order` | Text → WindowMove → Text を 3 push → undo 3 回 で pending に逆順の inverse が積まれる |

#### Bevy system レベルのテスト

`App::new()` に `AppHistory`, `PendingLayoutApply`, `StrategyBuffer`, `WindowManager` を init し、
ダミー `WindowRoot` 1 つを spawn → `pending.queue` に `MoveWindow` を push →
`apply_pending_app_edits_system` を 1 フレーム回し → `Transform.translation` が
更新されることを assert。`DespawnWindow` も同様。

---

### Sub-step 7.1.8 — メニューバーを dropdown 化し [ファイル] / [編集] を追加

#### 現状

[src/ui/menu_bar.rs:72-75](../../src/ui/menu_bar.rs#L72) を見ると、現行メニューは
**dropdown ではなくフラットなボタン列**:

```rust
spawn_menu_btn(p, "Open Strategy...", MenuButton::OpenStrategy);
spawn_menu_btn(p, "Save (Ctrl+S)", MenuButton::SaveLayout);
spawn_menu_btn(p, "Save As (Ctrl+Shift+S)", MenuButton::SaveLayoutAs);
spawn_menu_btn(p, "Load (Ctrl+O)", MenuButton::LoadLayout);
```

`MenuButton` enum + `menu_button_system` の `Interaction::Pressed` で分岐する pattern。

#### 目標構造

Phase 7.1 を機に **MS Office / VSCode 風の dropdown メニュー** に再編する。
既存のフラットボタンも全て dropdown 配下に移す:

```
┌─ メニューバー ──────────────────────────────────
│ [ファイル▾]  [編集▾]
└──────────────────────────────────────────────
   ↓ click "ファイル" でポップアップ
   ┌─────────────────────────────┐
   │ Open Strategy...            │
   │ Save              Ctrl+S    │
   │ Save As     Ctrl+Shift+S    │
   │ Load              Ctrl+O    │
   └─────────────────────────────┘

   ↓ click "編集" でポップアップ
   ┌─────────────────────────────┐
   │ 元に戻す          Ctrl+Z    │
   │ やり直し          Ctrl+Y    │
   └─────────────────────────────┘
```

#### データモデル

```rust
// src/ui/components.rs
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuTopLevel {
    File,
    Edit,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    OpenStrategy,
    SaveLayout,
    SaveLayoutAs,
    LoadLayout,
    Undo,
    Redo,
}

/// 現在開いている dropdown。None なら全て閉じている。
#[derive(Resource, Default, Debug)]
pub struct OpenMenu(pub Option<MenuTopLevel>);

/// dropdown ポップアップ本体（spawn 時に hidden、open 時に visible 切り替え）
#[derive(Component)]
pub struct MenuPopup(pub MenuTopLevel);
```

既存の `MenuButton` enum は `MenuItem` にリネームし、新たに親ボタン用 `MenuTopLevel` を追加。

#### 動作仕様

| イベント | 期待動作 |
|---------|---------|
| トップレベル `[ファイル]` クリック | `OpenMenu = Some(File)` ／ 既に開いていれば close |
| トップレベル `[編集]` クリック | `OpenMenu = Some(Edit)` ／ トグル |
| メニューが開いている状態で別トップレベルを hover | hover 先に切り替え（VSCode 風）。任意機能 |
| ポップアップ内アイテムクリック | アクション実行 → `OpenMenu = None` |
| ポップアップ外領域をクリック | `OpenMenu = None` |
| `Esc` キー | `OpenMenu = None` |

#### 実装ステップ

1. **ポップアップ spawn**: `spawn_menu_bar` で File / Edit ポップアップを `Visibility::Hidden` で
   `Node` ツリーとして仕込む。位置はトップレベルボタンの直下に absolute 配置。
2. **トップレベルクリック observer**:
   `Pointer<Click>` observer を `MenuTopLevel` 付きボタンに付け、`OpenMenu` を更新。
3. **可視性同期 system** (`Update`):
   ```rust
   pub fn sync_menu_popup_visibility_system(
       open: Res<OpenMenu>,
       mut q: Query<(&MenuPopup, &mut Visibility)>,
   ) {
       if !open.is_changed() { return; }
       for (popup, mut vis) in &mut q {
           *vis = if open.0 == Some(popup.0) { Visibility::Inherited } else { Visibility::Hidden };
       }
   }
   ```
4. **アイテムクリック**: 既存 `menu_button_system` を `MenuItem` に書き換え、
   末尾で `open_menu.0 = None` を呼んでメニューを閉じる。
5. **外側クリックで閉じる**:
   グローバル `Pointer<Click>` observer を background entity（カメラ or 全画面 sentinel）に付け、
   `OpenMenu = None` にする。トップレベル / ポップアップ自身のクリックは propagation を止めて
   外側クリック扱いにならないようにする。
6. **Esc**: 既存の入力 system に 1 行追加。

#### Undo/Redo アクションのフック

`menu_button_system` から Undo/Redo を呼ぶには、キー側 (`undo_redo_system`) と
共通のコア関数を `strategy_editor.rs` に切り出す:

```rust
pub fn perform_undo(
    history: &mut AppHistory,
    pending: &mut PendingAppEdits,
    applied_w: &mut EventWriter<UndoRedoApplied>,
) {
    history.replaying_depth = history.replaying_depth.saturating_add(1);
    let changed = history.record.undo(pending).is_some();
    if changed {
        applied_w.send(UndoRedoApplied);
    } else {
        // 何も pending に積まれなかったので depth を戻す。
        history.replaying_depth = history.replaying_depth.saturating_sub(1);
    }
    // ※ changed のときの depth 減算は apply_pending_app_edits_system が
    //    queue drain 完了時に行う（7.1.6 参照）。
}
pub fn perform_redo(/* 同様 */) { ... }
```

`undo_redo_system` も `menu_button_system` も同じ `perform_undo` / `perform_redo` を呼ぶ。

#### 視覚フィードバック (任意)

`history.record.can_undo()` / `can_redo()` が false のとき該当アイテムを薄表示にする:

```rust
pub fn update_menu_item_alpha_system(
    history: Res<AppHistory>,
    mut q: Query<(&MenuItem, &mut BackgroundColor)>,
) {
    for (item, mut bg) in &mut q {
        let enabled = match item {
            MenuItem::Undo => history.record.can_undo(),
            MenuItem::Redo => history.record.can_redo(),
            _ => true,
        };
        bg.0.set_alpha(if enabled { 1.0 } else { 0.4 });
    }
}
```

#### 段階分割の判断

dropdown 再編は Phase 7.1 のテキスト/ウィンドウ Undo 本体と独立して進められる。
リスク低減のため **以下 2 段階に分けて実装** することを推奨:

1. **7.1.8a — dropdown 基盤**: 既存 4 ボタンを [ファイル] dropdown 配下に移動。`[編集]` はまだ空。
2. **7.1.8b — [編集] メニュー追加**: `Undo` / `Redo` アイテムを [編集] に追加し `perform_undo`/`perform_redo` 配線。

7.1.8a が動かなくても 7.1.1〜7.1.7 のキー入力経由 Undo/Redo は機能するので、
仮に dropdown 化でハマっても本体のリリースをブロックしない。

#### 完了条件への追加

- [ ] メニューバーに [ファイル▾] [編集▾] のトップレベルボタンが並ぶ
- [ ] [ファイル] クリックで Open / Save / Save As / Load の dropdown が開く
- [ ] [編集] クリックで `元に戻す (Ctrl+Z)` / `やり直し (Ctrl+Y)` の dropdown が開く
- [ ] dropdown アイテムクリックで対応するアクションが実行され、メニューが閉じる
- [ ] dropdown 外領域クリック / Esc キーで dropdown が閉じる
- [ ] `can_undo()/can_redo()` が false のとき該当アイテムが薄表示になる（任意）

#### Caveat

- **z-order**: dropdown ポップアップは Floating Window より手前に出る必要がある。
  既存の `WindowManager.next_z` ベースの z 管理ではなく、UI ノード（`Node` / `ZIndex`）で
  `ZIndex::Global(i32::MAX)` のような最前面指定にする。
- **クリック透過**: 開いている dropdown の外をクリックして閉じるとき、その同じクリックが
  下のキャンバスや FloatingWindow に到達してドラッグを発火させてしまわないよう、
  ポップアップ open 中は外側クリック observer で `propagation` を止める。
- **再エントリ**: メニューから Undo を呼ぶと `perform_undo` 内で `OpenMenu = None` も
  発火させたいが、`menu_button_system` が Update 内で `OpenMenu` を書き換えた瞬間
  `sync_menu_popup_visibility_system` が同フレームで反応しない可能性がある。
  `.after(menu_button_system)` を明示する。

---

## ファイル変更一覧

| ファイル | 変更種別 | 内容 |
|----------|----------|------|
| `Cargo.toml` | 追記 | `undo = "0.52"` |
| `src/ui/editor_history.rs` | 新規 | `AppEdit` enum, `TextEdit`, `WindowMoveEdit`, `WindowSpawnEdit`, `WindowDespawnEdit`, `PendingAppEdits`, `AppEditAction`, `StrategySnapshot`, `AppHistory` Resource (`replaying_depth: u32`), `ActiveDrag` Resource, `PendingStrategySnapshotRestore` Resource, `UndoRedoApplied` Event, 単体テスト |
| `src/ui/components.rs` | 変更 | `PanelSpawnRequested` に `source: PanelSpawnSource` を追加 (`User` / `LayoutLoad` / `UndoRedo`)。Undo/Redo 関連の Resource/Event は editor_history.rs 側に置く (Findings 5) |
| `src/ui/strategy_editor.rs` | 変更 | `undo_redo_system` 追加 / `sync_editor_to_strategy_buffer_system` で `AppEdit::Text` を push / `sync_strategy_buffer_to_editor_system` を `UndoRedoApplied` でも動かす / `apply_pending_app_edits_system` 追加 |
| `src/ui/floating_window.rs` | 変更 | TitleBar に `Pointer<DragStart>` / `Pointer<DragEnd>` observer を追加（before/after 位置を `ActiveDrag` 経由で取得）/ CloseButton の Click observer を拡張して `AppEdit::WindowDespawn` を push / `panel_spawn_dispatcher_system` で `event.source == User` のときだけ `AppEdit::WindowSpawn` を push |
| `src/ui/layout_persistence.rs` | 変更 | `apply_layout_system` の `PanelSpawnRequested` 発火に `source: PanelSpawnSource::LayoutLoad` を付与 / `mark_dirty_on_drag_system` 等の dirty 立て系 system 全てに `if history.is_replaying() { return; }` ガード追加 (Findings 5) |
| `src/ui/sidebar.rs` | 変更 | `PanelSpawnRequested` 発火 2 箇所に `source: PanelSpawnSource::User` を付与 |
| `src/ui/menu_bar.rs` | 変更 | dropdown 化（`MenuButton` → `MenuItem` rename、`MenuTopLevel` 追加、ポップアップ spawn、open/close observer、`MenuItem::Undo`/`Redo` 分岐で `perform_undo`/`perform_redo` 呼出、`open_strategy_buffer_system` 末尾で `AppHistory` リセット） |
| `src/ui/components.rs` | 追加 | `MenuTopLevel`, `MenuItem`, `MenuPopup`, `OpenMenu` Resource |
| `src/ui/mod.rs` | 変更 | `mod editor_history;`, `init_resource::<AppHistory>()`, `init_resource::<ActiveDrag>()`, `init_resource::<OpenMenu>()`, `add_event::<UndoRedoApplied>()`, `sync_menu_popup_visibility_system` 登録, system 登録順序 |

---

## 既知 Caveat

1. **`replaying_depth` のライフタイム (Findings 2 対応)**
   `apply_pending_app_edits_system` が `buffer.source` や `Transform` を書き換えると、
   `sync_strategy_buffer_to_editor_system` の `set_text` / Drag observer の `DragEnd` /
   `panel_spawn_dispatcher_system` の重複 spawn 検出 などが**翌フレーム以降**で発火しうる。
   これらが「ユーザー操作」と誤認されて history に再 push されると履歴が汚染される。

   対応:
   - `AppHistory.replaying_depth: u32` カウンタを採用。`undo_redo_system` で `+= 1`、
     `apply_pending_app_edits_system` が queue を完全に drain した直後に `-= 1`。
   - 各 push 経路（テキスト・WindowMove・WindowSpawn・WindowDespawn）では
     `if history.is_replaying() { return; }` で弾く。
   - 加えて Findings 3 対応で `PanelSpawnRequested.source` を導入し、
     dispatcher で `source == User` のときだけ history に push する二重ガードを敷く。

   **残リスク**: `DragEnd` が翌フレームに来るケースなど、`apply_pending_app_edits_system` の
   完了より遅延して中間イベントが届く可能性は理論上ある。実装時に問題が顕在化したら
   「次フレーム冒頭まで depth を保持」する `pending_replaying_decrement: bool` を追加検討。

2. **CosmicEditor の set_text タイミング** (memory: cosmic-edit Buffer メトリクスの DPI トラップ)
   Undo 後のエディタ反映は `CosmicEditBuffer` と `CosmicEditor` 両方に `set_text` が必要。
   [src/ui/strategy_editor.rs:288-303](../../src/ui/strategy_editor.rs#L288) で両方更新済み。

3. **「別ファイル」Open 時のみ履歴リセット (Findings 3 対応)**
   `open_strategy_buffer_system` 末尾で履歴リセットを行うが、**`buffer.original_path` が
   ロード先 path と異なるときのみ**に限定する。Strategy Editor パネルを開くフロー
   （`PanelSpawnRequested → PendingStrategyLoad → OpenStrategyRequested`）では同じ
   path を再ロードする可能性があり、無条件リセットだと直前の `WindowSpawn` 履歴を
   消してしまう（→「Ctrl+Z で開いたばかりのパネルが閉じる」が壊れる）。
   実装スケッチは 7.1.6 末尾参照。

4. **`Instant` は serde 非対応 / 履歴は永続化しない**
   `TextEdit.timestamp` に `std::time::Instant` を使うため serde 不可。
   **Undo 履歴は永続化対象外**（Phase 7.7 の sidecar layout には保存しない）。
   セッションを跨いだ undo は仕様外。

5. **再 spawn された Entity は新 ID になる**
   `DespawnWindow` を undo して再 spawn したパネルは、despawn 前の `Entity` ID と異なる。
   そのため「古い Entity ID をどこかが保持している」状態は壊れる。幸い現状の
   コードベースでは Entity ID をリソースに持つ箇所はなく、すべて `PanelKind`
   ベースで照合しているので問題ない。**今後 Entity ID をリソースに保持する場合は
   undo 経由の再 spawn を考慮する必要がある**。

6. **Strategy Editor の despawn → undo 時のテキスト復元 (Findings 4 対応済)**
   ユーザーが未保存編集中の Strategy Editor を × で閉じて Ctrl+Z で開き直したとき、
   **disk 内容ではなく閉じる直前の `buffer.source` を復元する**。

   実装は **2 段階遅延** で行う:
   1. `apply_pending_app_edits_system` が `SpawnWindow` を drain したフレーム:
      `PanelSpawnRequested` を発火 + `PendingStrategySnapshotRestore` キューに snapshot を積む。
      この時点ではエディタ entity がまだ存在しないため、`buffer.source` の書き戻しはしない。
   2. 次フレーム以降: `panel_spawn_dispatcher_system` がエディタを spawn →
      `apply_strategy_snapshot_restore_system` が entity 存在を確認してから
      `buffer.source` 書き戻し + `UndoRedoApplied` 発火。

   `sidebar.rs:237` の `PendingStrategyLoad` と同じパターン。

7. **ドラッグの中断**
   ユーザーが DragStart したまま app が落ちた / フォーカスが外れて DragEnd が来ない
   ケースでは `ActiveDrag.starts` に古いエントリが残るが、メモリリーク以外の害はない。
   許容する。

8. **`Pointer<Drag>` の既存 observer は残す**
   リアルタイムにウィンドウを追従させるため、現行の `transform.translation += delta`
   observer は撤去しない。新規追加するのは DragStart（before_pos 記録）と
   DragEnd（before/after 比較して push）のみ。

9. **Layout autosave との相互作用 (Findings 5 / OQ2 対応)**
   現行 [layout_persistence.rs](../../src/ui/layout_persistence.rs) には
   ウィンドウの spawn/despawn/move を検知して sidecar を auto-save する経路がある。
   Undo/Redo 中に発生する一時的な despawn → spawn / position 巻き戻しが auto-save を
   発火すると、**「履歴再生中の中間状態」が sidecar に保存される** 危険がある。

   対応: layout dirty を立てる **全経路** に `if history.is_replaying() { return; }` ガード追加。
   実装着手前に以下を grep して洗い出し、計画書のチェックリストに反映する:

   ```text
   rg -n "dirty\s*=\s*true|set_dirty|layout_dirty" src/ui/layout_persistence.rs
   ```

   現時点で確認済みの経路:
   - `mark_dirty_on_drag_system` ([layout_persistence.rs:379](../../src/ui/layout_persistence.rs#L379)) — DragEnd
   - パネル close 検知系（despawn 監視 / `RemovedComponents<WindowRoot>` 等を使う system があれば該当）
   - パネル spawn 検知系（`Added<WindowRoot>` 等を使う system があれば該当）

   ガード未敷設の経路が見つかった場合はファイル変更一覧の `layout_persistence.rs` 欄に追記。

10. **テキスト merge ポリシーの境界 (Open Question 1)**
    通常入力同士は 500ms / 単語境界でマージ。**paste（一括ペースト）と複数行 edit は
    常に独立した entry にする**:
    `TextEdit.merge` 内で `after.len() - before.len() > 50`（経験値、要調整）または
    差分が 50 文字超または差分に `\n` 追加が含まれるとき `Merged::No(other)` を返す（7.1.2 の `merge` 実装参照）。
    paste をひとつの undo 単位にしたいユーザー体験と一致する。CosmicEditor からは
    `CosmicTextChanged` で全文しか届かないため、diff size での判定が最も簡便。

---

## 完了条件

- [ ] Ctrl+Z で直前のテキスト編集が戻る
- [ ] Ctrl+Z でドラッグ移動が戻る（移動前の位置に瞬時にジャンプ）
- [ ] Ctrl+Z で開いたばかりのパネルが閉じる（spawn の undo）
- [ ] Ctrl+Z で閉じたパネルが再表示される（despawn の undo、位置・サイズ・z を含めて復元）
- [ ] Strategy Editor を未保存編集状態で閉じて Ctrl+Z すると、編集内容が **2 段階遅延** で正しく復元される（spawn 完了次フレームに snapshot 流し込み）
- [ ] Ctrl+Z / Ctrl+Y を連打しても auto-save が dirty を立てて中間状態を sidecar に書かない
- [ ] Ctrl+Y と Ctrl+Shift+Z の **両方** がやり直しキーとして機能し、テキスト・移動・spawn・despawn いずれにも効く
- [ ] テキスト・移動・spawn・despawn が混在した履歴で Ctrl+Z を連打すると逆順で 1 件ずつ巻き戻る
- [ ] 連続テキスト入力は 500ms または単語境界で 1 undo 単位にまとまる
- [ ] 1 回のドラッグは 1 undo 単位（中間ピクセルは履歴に積まない）
- [ ] 別ファイルを開くと履歴がリセットされる
- [ ] `Record::builder().limit(200).build()` で履歴が 200 件に制限されている
- [ ] `AppEdit::merge` の単体テスト群（テキスト merge / 各境界文字 / ウィンドウ系 No / 異種 No）が pass
- [ ] `AppEdit::edit/undo` が `PendingAppEdits` に正しいアクションを積む単体テストが pass
- [ ] `apply_pending_app_edits_system` で Transform / despawn / spawn 要求が正しく発火する system テストが pass
- [ ] `cargo check` / `cargo test` が通る
- [ ] E2E: test_strategy_daily.py を開いて編集 → パネルをドラッグ → 別パネルを開く → 閉じる →
      Ctrl+Z を 4 回押して全操作が逆順に戻ることを目視確認

# Plan: Phase 7.1 Missed — Menu Bar Dropdown + Undo/Redo

## Context

Phase 7.1 でキーボード Undo/Redo は実装済みだが、メニューバー変更が漏れていた。
追加要件: ドロップダウン形式に変更 + "Open Strategy..." ボタンを削除。

**目標状態:**
```
┌ MenuBar ─────────────────────────────────────
│ [ファイル ▾]  [編集 ▾]         strategy: none
│
│ ▼ ファイル popup (クリック時)    ▼ 編集 popup (クリック時)
│   Save (Ctrl+S)                  Undo (Ctrl+Z)
│   Save As (Ctrl+Shift+S)         Redo (Ctrl+Y)
│   Load (Ctrl+O)
```

`OpenStrategyRequested` イベントは起動時の自動復元に使われているため**保持**。ボタンだけ削除。

---

## Files to Modify

| File | 変更内容 |
|------|---------|
| `src/ui/components.rs` | `MenuButton` 削除 → `MenuTopLevel`, `MenuItem`, `MenuPopup`, `OpenMenu` 追加。`UndoMenuRequested`/`RedoMenuRequested` イベント追加 |
| `src/ui/menu_bar.rs` | `spawn_menu_bar` をドロップダウン構造に全面書き換え。`menu_button_system` → `menu_top_level_system` + `menu_item_system` + `sync_menu_popup_visibility_system` |
| `src/ui/strategy_editor.rs` | `undo_redo_system` に `EventReader<UndoMenuRequested/RedoMenuRequested>` 追加 |
| `src/ui/mod.rs` | システム/イベント/リソース登録を更新 |

---

## Step-by-Step

### Step 1 — `src/ui/components.rs`

**削除:** `MenuButton` enum (lines 57-63)

**追加:**
```rust
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTopLevel {
    File,
    Edit,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    SaveLayout,
    SaveLayoutAs,
    LoadLayout,
    Undo,
    Redo,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuPopup(pub MenuTopLevel);

#[derive(Resource, Default)]
pub struct OpenMenu(pub Option<MenuTopLevel>);

#[derive(Event, Debug, Clone)]
pub struct UndoMenuRequested;

#[derive(Event, Debug, Clone)]
pub struct RedoMenuRequested;
```

### Step 2 — `src/ui/menu_bar.rs` (大幅書き換え)

**imports 変更:**
- `MenuButton` を削除、`MenuTopLevel, MenuItem, MenuPopup, OpenMenu, UndoMenuRequested, RedoMenuRequested` を追加

**新ヘルパー:**
```rust
fn spawn_menu_item(parent: &mut ChildBuilder, label: &str, action: MenuItem) {
    // spawn_menu_btn と同じスタイルで MenuItem コンポーネントを付ける
}
```

**`spawn_menu_bar` の新構造:**
```rust
pub fn spawn_menu_bar(mut commands: Commands) {
    commands.spawn((Node { /* 現行と同じ row layout */ }, BackgroundColor(...), MenuBarRoot))
        .with_children(|p| {
            // [ファイル ▾] top-level button
            p.spawn((Button, Node { overflow: Overflow::visible(), padding: ..., position_type: PositionType::Relative, ..default() },
                     BackgroundColor(BTN_NORMAL), MenuTopLevel::File))
                .with_children(|p| {
                    p.spawn((Text::new("ファイル ▾"), TextFont { font_size: 12.0, ..default() },
                              TextColor(Color::srgb(0.82, 0.82, 0.82))));
                    // popup panel (hidden by default)
                    p.spawn((Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        top: Val::Px(22.0),
                        left: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        min_width: Val::Px(180.0),
                        ..default()
                    }, BackgroundColor(Color::srgba(0.10, 0.10, 0.16, 0.98)),
                       ZIndex(100), MenuPopup(MenuTopLevel::File)))
                        .with_children(|p| {
                            spawn_menu_item(p, "Save (Ctrl+S)", MenuItem::SaveLayout);
                            spawn_menu_item(p, "Save As (Ctrl+Shift+S)", MenuItem::SaveLayoutAs);
                            spawn_menu_item(p, "Load (Ctrl+O)", MenuItem::LoadLayout);
                        });
                });

            // [編集 ▾] top-level button
            p.spawn((Button, Node { overflow: Overflow::visible(), ..default() },
                     BackgroundColor(BTN_NORMAL), MenuTopLevel::Edit))
                .with_children(|p| {
                    p.spawn((Text::new("編集 ▾"), ...));
                    p.spawn((Node { display: Display::None, position_type: PositionType::Absolute,
                                    top: Val::Px(22.0), flex_direction: FlexDirection::Column, ..default() },
                              BackgroundColor(...), ZIndex(100), MenuPopup(MenuTopLevel::Edit)))
                        .with_children(|p| {
                            spawn_menu_item(p, "Undo (Ctrl+Z)", MenuItem::Undo);
                            spawn_menu_item(p, "Redo (Ctrl+Y)", MenuItem::Redo);
                        });
                });

            // spacer
            p.spawn(Node { flex_grow: 1.0, ..default() });

            // strategy status label (現行と同じ)
            p.spawn((Text::new("strategy: none"), ..., StrategyStatusLabel));
        });
}
```

**新システム:**

```rust
// トップレベルボタンのクリック → OpenMenu トグル
pub fn menu_top_level_system(
    mut query: Query<(&Interaction, &mut BackgroundColor, &MenuTopLevel), (Changed<Interaction>, With<Button>)>,
    mut open_menu: ResMut<OpenMenu>,
) {
    for (interaction, mut bg, top) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                open_menu.0 = if open_menu.0 == Some(*top) { None } else { Some(*top) };
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

// popup の Display を OpenMenu に同期
pub fn sync_menu_popup_visibility_system(
    open_menu: Res<OpenMenu>,
    mut popup_q: Query<(&MenuPopup, &mut Node)>,
) {
    if !open_menu.is_changed() { return; }
    for (popup, mut node) in &mut popup_q {
        node.display = if open_menu.0 == Some(popup.0) { Display::Flex } else { Display::None };
    }
}

// メニューアイテムのクリック → イベント発火 + メニューを閉じる
pub fn menu_item_system(
    mut query: Query<(&Interaction, &mut BackgroundColor, &MenuItem), (Changed<Interaction>, With<Button>)>,
    mut open_menu: ResMut<OpenMenu>,
    mut save_ev: EventWriter<LayoutSaveRequested>,
    mut save_as_ev: EventWriter<LayoutSaveAsRequested>,
    mut load_ev: EventWriter<LayoutLoadDialogRequested>,
    mut undo_ev: EventWriter<UndoMenuRequested>,
    mut redo_ev: EventWriter<RedoMenuRequested>,
) {
    for (interaction, mut bg, item) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                open_menu.0 = None;  // close menu on item click
                match item {
                    MenuItem::SaveLayout => save_ev.send(LayoutSaveRequested),
                    MenuItem::SaveLayoutAs => save_as_ev.send(LayoutSaveAsRequested),
                    MenuItem::LoadLayout => load_ev.send(LayoutLoadDialogRequested),
                    MenuItem::Undo => undo_ev.send(UndoMenuRequested),
                    MenuItem::Redo => redo_ev.send(RedoMenuRequested),
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}
```

**削除:** `menu_button_system`, `spawn_menu_btn` (名前変更して `spawn_menu_item` に)

### Step 3 — `src/ui/strategy_editor.rs` (~line 367)

`undo_redo_system` のシグネチャに追加:
```rust
mut undo_menu_ev: EventReader<UndoMenuRequested>,
mut redo_menu_ev: EventReader<RedoMenuRequested>,
```

関数冒頭でイベント読み取り:
```rust
let menu_undo = undo_menu_ev.read().next().is_some();
let menu_redo = redo_menu_ev.read().next().is_some();
```

cooldown チェックを修正 (メニューイベントはクールダウン無視):
```rust
if *cooldown > 0.0 && !menu_undo && !menu_redo {
    return;
}
```

ctrl チェックを修正 (メニューイベントは ctrl 不要):
```rust
let do_undo = menu_undo || (ctrl && keys.just_pressed(KeyCode::KeyZ) && !shift);
let do_redo = menu_redo || (ctrl && (keys.just_pressed(KeyCode::KeyY)
    || (keys.just_pressed(KeyCode::KeyZ) && shift)));
```

既存の `if !ctrl { return; }` ガードは **削除** する (ctrl チェックは上記 do_undo/do_redo に内包)。

use 文に追加:
```rust
use crate::ui::components::{UndoMenuRequested, RedoMenuRequested, /* 既存 */};
```

### Step 4 — `src/ui/mod.rs`

**import 変更:**
- `MenuButton` → `MenuTopLevel, MenuItem, MenuPopup, OpenMenu, UndoMenuRequested, RedoMenuRequested`
- `menu_button_system` → `menu_top_level_system, menu_item_system, sync_menu_popup_visibility_system`

**`build()` 変更:**
```rust
.init_resource::<OpenMenu>()
.add_event::<UndoMenuRequested>()
.add_event::<RedoMenuRequested>()
```

Update system 登録:
- `menu_button_system` を削除
- 追加: `menu_top_level_system, menu_item_system, sync_menu_popup_visibility_system`

---

## 注意点

- `ZIndex(100)` で popup を他 UI より前面に出す。Bevy バージョンによっては `GlobalZIndex(100)` が必要な場合あり → Navigator が Cargo.toml で Bevy バージョンを確認すること
- `overflow: Overflow::visible()` をトップレベルボタンノードに設定しないと popup が clip される
- `MenuBarRoot` オーバーフローも `Overflow::visible()` にする必要あり
- `OpenStrategyRequested` イベントと関連システムはすべて保持 (起動時の自動復元で使用)
- `menu_bar.rs` の `open_strategy_buffer_system` 等のファイルストラテジー関連システムは変更なし

---

## Verification

1. `cargo check` — zero errors
2. `cargo test` — all pass
3. E2E: ファイルドロップダウンクリック → Save/Save As/Load が動作する
4. E2E: 編集ドロップダウン → Undo/Redo が動作する
5. E2E: キーボード Ctrl+Z / Ctrl+Y は引き続き動作する
6. E2E: "Open Strategy..." ボタンが存在しないことを確認
