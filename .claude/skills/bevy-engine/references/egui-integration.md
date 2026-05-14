# bevy_egui 連携（0.31, Bevy 0.15）

## 基本形

```rust
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, egui};

// Plugin 登録は UiPlugin で一度だけ:
//   app.add_plugins(EguiPlugin);

fn my_panel_system(mut contexts: EguiContexts, mut state: ResMut<MyState>) {
    egui::Window::new("Title")
        .default_width(800.0)
        .default_height(600.0)
        .show(contexts.ctx_mut(), |ui| {
            ui.label(&state.message);
            if ui.button("Click").clicked() { state.count += 1; }
        });
}
```

## レイアウト・ウィジェット早見

```rust
ui.horizontal(|ui| { ui.label("a"); ui.label("b"); });
ui.vertical(|ui| { /* ... */ });
ui.separator();

// ボタン（有効/無効を動的に切替）
let r = ui.add_enabled(can_save, egui::Button::new("Save"));
if r.clicked() { /* ... */ }

// 複数行テキスト
ui.add(egui::TextEdit::multiline(&mut buf)
    .desired_width(f32::INFINITY)
    .desired_rows(30));

// スクロール
egui::ScrollArea::vertical().show(ui, |ui| { /* ... */ });

// テーブル風
egui::Grid::new("g").show(ui, |ui| {
    ui.label("col1"); ui.label("col2"); ui.end_row();
});
```

## change detection に注意（このプロジェクトの実害ケース）

```rust
// ❌ DerefMut のたびに resource を「変更された」とマークしてしまう
fn bad(mut buf: ResMut<StrategyBuffer>) {
    egui::TextEdit::multiline(&mut buf.source).show(ui);
    // buf.source を編集していなくても毎フレーム変更扱い → 下流の system が無駄に走る
}

// ✅ ローカルにクローン、変化したときだけ書き戻す
fn good(mut buf: ResMut<StrategyBuffer>) {
    let mut source = buf.source.clone();
    let resp = ui.add(egui::TextEdit::multiline(&mut source));
    if resp.changed() {
        buf.source = source;
        buf.dirty = true;
    }
}
```

`strategy_editor.rs:73-80` がこのパターン。**egui の中で `ResMut<T>` の内側を直接
編集するのは避ける** が原則。

## ctx を取り出すときの罠

`EguiContexts` は内部で `Query` を持っており、複数 window がある環境では
`contexts.ctx_mut()` は primary window の ctx を返す。本プロジェクトは
single-window なので気にしなくていいが、ウィンドウを増やすときは
`contexts.ctx_for_window_mut(entity)` を使う。

## bevy_egui と Bevy world-space ウィンドウの干渉

egui が hover している間は **Bevy 側の Pointer event がブロックされない** ことに
注意。`bevy_egui::EguiSet` 周りの system 順序を `.before(EguiSet::ProcessInput)`
で挟む必要が出る場合がある。今のところ本プロジェクトでは egui ウィンドウと
world-space ウィンドウの重なりで実害が出ていないが、もし「egui の上でも背後の
sprite がドラッグできてしまう」となったら以下のガードを system 先頭に入れる:

```rust
fn drag_system(
    mut contexts: EguiContexts,
    /* ... */
) {
    if contexts.ctx_mut().wants_pointer_input() {
        return; // egui 側にポインタを譲る
    }
    // 通常処理
}
```

## バージョンメモ

- bevy_egui 0.31 = Bevy 0.15 対応
- bevy_egui 0.32+ = Bevy 0.16 以降
- API 名（`EguiContexts::ctx_mut` 等）は 0.30 系から大きく変わっていないが、
  `Egui*Set` のスケジュール名は時々変わる
