//! I11 edit_menu_undo_redo — Edit→Undo / Edit→Redo が `AppHistory` の直前操作を
//! 取り消し・やり直しすることを保証する（kind:ui）。
//!
//! # 駆動経路
//! `MenuItem::Undo` / `MenuItem::Redo` に `Interaction::Pressed` を注入 →
//! `menu_item_system` が `UndoMenuRequested` / `RedoMenuRequested` を発火 →
//! `undo_redo_system` が `AppHistory.record` に対して undo/redo を呼ぶ →
//! `AppHistory.pending.queue` に `SetStrategySource` アクションが積まれる。
//!
//! # 観測
//! - undo 後: `pending.queue` に `before` テキストが積まれていること
//! - redo 後: `pending.queue` に `after` テキストが積まれていること
//! - 空の履歴に対する undo/redo は no-op（パニックしない）
//!
//! # テスト設計
//! `undo_redo_system` は `Time` の delta を使ってクールダウンを減算する。
//! bare `App` の `Time<()>` は `advance_by(1s)` で delta=1 秒にして
//! クールダウンを確実に解除してから各操作を行う。

use std::time::Duration;

use bevy::prelude::*;

use backcast::ui::components::{MenuItem, OpenMenu, UndoMenuRequested, RedoMenuRequested};
use backcast::ui::editor_history::{AppEdit, AppHistory, TextEdit};
use backcast::ui::layout_persistence::{
    LayoutLoadDialogRequested, LayoutSaveAsRequested, LayoutSaveRequested,
};
use backcast::ui::menu_bar::menu_item_system;
use backcast::ui::strategy_editor::undo_redo_system;
use backcast::trading::{ExecutionModeRes, VenueStatusRes};

fn build_app() -> App {
    let mut app = App::new();

    app.insert_resource(ExecutionModeRes::default());
    // menu_item_system が VenueStatusRes を要求する。
    app.insert_resource(VenueStatusRes::default());
    app.insert_resource(OpenMenu::default());
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(Time::<()>::default());
    app.insert_resource(AppHistory::default());

    app.add_event::<LayoutSaveRequested>();
    app.add_event::<LayoutSaveAsRequested>();
    app.add_event::<LayoutLoadDialogRequested>();
    app.add_event::<UndoMenuRequested>();
    app.add_event::<RedoMenuRequested>();

    // menu_item_system → undo_redo_system の順でチェーン。
    app.add_systems(Update, (menu_item_system, undo_redo_system).chain());

    app
}

/// AppHistory に TextEdit を 1 件 push する。
/// push_text は通常入力路で pending をクリアするため、Record に直接積む。
fn push_text_edit(app: &mut App, before: &str, after: &str) {
    let mut history = app.world_mut().resource_mut::<AppHistory>();
    let edit = AppEdit::Text(TextEdit {
        region_key: "region_001".to_string(),
        before: before.to_string(),
        after: after.to_string(),
        timestamp: std::time::Instant::now(),
    });
    // record.edit は pending に push するが、通常入力時は pending を clear する設計。
    let AppHistory { record, pending, .. } = &mut *history;
    record.edit(pending, edit);
    history.pending.queue.clear();
}

/// 1 秒進めてクールダウンを解除してから、ボタンを spawn して 1 フレーム回す。
fn press_menu(app: &mut App, item: MenuItem) {
    app.world_mut()
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_secs(1));
    app.world_mut().spawn((
        Button,
        Interaction::Pressed,
        BackgroundColor::default(),
        item,
    ));
    app.update();
}

#[test]
fn i11_edit_menu_undo_redo() {
    // ── ケース 1: Undo → pending に before が積まれる ──
    {
        let mut app = build_app();
        push_text_edit(&mut app, "before_text", "after_text");

        assert_eq!(
            app.world().resource::<AppHistory>().record.len(),
            1,
            "push 後は record に 1 件あるはず"
        );

        press_menu(&mut app, MenuItem::Undo);

        let history = app.world().resource::<AppHistory>();
        // undo が成功すると replaying_depth > 0 または pending に before が積まれる。
        // undo_redo_system は record.undo を呼び、pending に SetStrategySource(before) を積む。
        assert!(
            !history.pending.queue.is_empty(),
            "Undo 後は pending.queue に before アクションが積まれるはず"
        );

        match &history.pending.queue[0] {
            backcast::ui::editor_history::AppEditAction::SetStrategySource { text, region_key } => {
                assert_eq!(text, "before_text", "Undo で before_text が pending されるはず");
                assert_eq!(region_key, "region_001");
            }
            other => panic!("SetStrategySource を期待したが got {:?}", other),
        }
    }

    // ── ケース 2: Undo → Redo → pending に after が積まれる ──
    {
        let mut app = build_app();
        push_text_edit(&mut app, "v1", "v2");

        // Undo
        press_menu(&mut app, MenuItem::Undo);
        // pending をクリア（apply_pending_app_edits_system 相当）して次のアクションを観測可能にする。
        app.world_mut()
            .resource_mut::<AppHistory>()
            .pending
            .queue
            .clear();

        // undo 後は replaying_depth を reset する必要がある（本番は apply_pending_app_edits_system が担当）。
        app.world_mut()
            .resource_mut::<AppHistory>()
            .replaying_depth = 0;

        // Redo
        press_menu(&mut app, MenuItem::Redo);

        let history = app.world().resource::<AppHistory>();
        assert!(
            !history.pending.queue.is_empty(),
            "Redo 後は pending.queue に after アクションが積まれるはず"
        );
        match &history.pending.queue[0] {
            backcast::ui::editor_history::AppEditAction::SetStrategySource { text, .. } => {
                assert_eq!(text, "v2", "Redo で v2 が pending されるはず");
            }
            other => panic!("SetStrategySource を期待したが got {:?}", other),
        }
    }

    // ── ケース 3: 空の履歴に Undo しても no-op（パニックしない、replaying_depth はゼロのまま）──
    {
        let mut app = build_app();
        // 何も push しない状態で Undo
        press_menu(&mut app, MenuItem::Undo);

        let history = app.world().resource::<AppHistory>();
        assert_eq!(
            history.record.len(),
            0,
            "空の履歴では record が空のまま"
        );
        // 変更なしの場合は replaying_depth がすぐ戻る（undo_redo_system 内で即 -1）。
        assert_eq!(
            history.replaying_depth, 0,
            "空の履歴への Undo 後は replaying_depth = 0 のまま"
        );
        assert!(
            history.pending.queue.is_empty(),
            "空の履歴への Undo では pending に何も積まれない"
        );
    }
}
