//! K9 order_context_modify_cancel — working order の context menu から訂正・取消を開始し、
//! modal confirm 後に modify/cancel command が送られることを保証する（kind:ui）。
//!
//! テストでは Orders row context interaction / modal input / confirm を注入し、transport command と modal visibility を観測する。
//!
//! 検証フロー:
//! 1. メニュー open 状態で ContextMenuItem::Cancel → CancelOrder コマンド送信 + メニューが閉じる。
//! 2. メニュー open 状態で ContextMenuItem::Modify → ModifyForm が開く + メニューが閉じる（コマンドは送らない）。
//! 3. メニューが closed の状態でのアイテムクリックは no-op（コマンド送信なし）。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{SecretPrompt, TransportCommand, TransportCommandSender};
use backcast::ui::modify_modal::ModifyForm;
use backcast::ui::order_context_menu::{
    context_menu_item_system, ContextMenuItem, OrderContextMenu,
};
use backcast::ui::order_panel::OrderConfirm;

fn build_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(OrderContextMenu::default());
    app.insert_resource(ModifyForm::default());
    app.insert_resource(SecretPrompt::default());
    app.insert_resource(OrderConfirm::default());
    app.insert_resource(TransportCommandSender { tx });
    app.add_systems(Update, context_menu_item_system);

    (app, rx)
}

/// メニューを open 状態にする（注文 ID と venue を設定）。
fn open_menu(app: &mut App, order_id: &str, venue: &str) {
    let mut menu = app.world_mut().resource_mut::<OrderContextMenu>();
    menu.open = true;
    menu.client_order_id = Some(order_id.to_string());
    menu.venue = venue.to_string();
    menu.screen_pos = Vec2::new(100.0, 200.0);
}

#[test]
fn k9_order_context_modify_cancel() {
    // ── Case 1: Cancel アイテムクリック → CancelOrder 送信 + メニューが閉じる ────
    {
        let (mut app, mut rx) = build_app();
        open_menu(&mut app, "order-abc", "MOCK");

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();

        let cmd = rx
            .try_recv()
            .expect("Cancel item must fire CancelOrder command");
        match cmd {
            TransportCommand::CancelOrder {
                venue,
                order_id,
                second_secret,
            } => {
                assert_eq!(venue, "MOCK", "venue must match the open menu's venue");
                assert_eq!(order_id, "order-abc", "order_id must match client_order_id");
                assert!(
                    second_secret.is_none(),
                    "OrderPanel Cancel does not carry second_secret (Step 5)"
                );
            }
            other => panic!("expected CancelOrder, got {other:?}"),
        }

        let menu = app.world().resource::<OrderContextMenu>();
        assert!(!menu.open, "context menu must close after Cancel item click");
        assert!(
            menu.client_order_id.is_none(),
            "client_order_id must be cleared on close"
        );
    }

    // ── Case 2: Modify アイテムクリック → ModifyForm が開く + メニューが閉じる ───
    {
        let (mut app, mut rx) = build_app();
        open_menu(&mut app, "order-xyz", "MOCK");

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Modify));
        app.update();

        // Modify はコマンドを送らない（モーダルを開くだけ）。
        assert!(
            rx.try_recv().is_err(),
            "Modify item must NOT fire a command — opens ModifyForm instead"
        );

        let form = app.world().resource::<ModifyForm>();
        assert!(form.open, "Modify item must open ModifyForm");
        assert_eq!(
            form.client_order_id, "order-xyz",
            "ModifyForm.client_order_id must be set from context menu"
        );
        assert_eq!(
            form.venue, "MOCK",
            "ModifyForm.venue must be set from context menu"
        );

        let menu = app.world().resource::<OrderContextMenu>();
        assert!(!menu.open, "context menu must close after Modify item click");
    }

    // ── Case 3: メニューが closed の状態でのアイテムクリックは no-op ──────────────
    {
        let (mut app, mut rx) = build_app();
        // open_menu を呼ばない → open = false のまま

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "closed menu must not fire CancelOrder"
        );

        let menu = app.world().resource::<OrderContextMenu>();
        assert!(
            !menu.open,
            "closed menu must stay closed after stale item click"
        );
    }

    // ── Case 4: client_order_id が None でも Cancel クリック → メニューが閉じるだけ ─
    // (open=true だが order_id は None という不整合状態への防御)
    {
        let (mut app, mut rx) = build_app();
        {
            let mut menu = app.world_mut().resource_mut::<OrderContextMenu>();
            menu.open = true;
            // client_order_id は None のまま
            menu.venue = "MOCK".to_string();
        }

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();

        // client_order_id が None の場合、context_menu_item_system は close して return する。
        assert!(
            rx.try_recv().is_err(),
            "Cancel with no client_order_id must not fire a command"
        );
        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "menu must still close even when client_order_id is None"
        );
    }
}
