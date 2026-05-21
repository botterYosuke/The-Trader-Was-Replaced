//! K16 order_context_menu_open_close — Orders 行の右クリックで context menu が開き、
//! backdrop click / Escape で閉じ、閉じた状態の menu item click は no-op になることを保証する（kind:ui）。
//!
//! テストでは order row context interaction / backdrop / Escape / stale item click を注入し、context menu state と command 未送信を観測する。
//!
//! 検証フロー:
//! 1. `OrderContextMenu.open = true` にしたとき visibility system が Display::Flex に同期する。
//! 2. backdrop click → menu.open = false に変わる（閉じる）。
//! 3. Escape (higher-priority modal なし) → menu が閉じる。
//! 4. Escape + SecretPrompt.active = Some → menu は閉じない（優先度ロジック）。
//! 5. Escape + OrderConfirm.pending = Some → menu は閉じない（優先度ロジック）。
//! 6. Escape + ModifyForm.open = true → menu は閉じない（優先度ロジック）。
//! 7. menu closed の状態での item click → no-op（コマンド送信なし）。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{SecretPrompt, SecretPromptRequest, TransportCommand, TransportCommandSender};
use backcast::ui::modify_modal::ModifyForm;
use backcast::ui::order_context_menu::{
    context_menu_item_system, context_menu_keyboard_system, context_menu_visibility_system,
    ContextMenuBackdrop, ContextMenuItem, ContextMenuPanel, ContextMenuRoot, OrderContextMenu,
};
use backcast::ui::order_panel::{OrderConfirm, OrderDraft, OrderType, Side, TimeInForce};

fn build_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(OrderContextMenu::default());
    app.insert_resource(ModifyForm::default());
    app.insert_resource(SecretPrompt::default());
    app.insert_resource(OrderConfirm::default());
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(TransportCommandSender { tx });

    app.add_systems(
        Update,
        (
            context_menu_visibility_system,
            context_menu_keyboard_system,
            context_menu_item_system,
        ),
    );

    (app, rx)
}

fn open_menu(app: &mut App) {
    let mut menu = app.world_mut().resource_mut::<OrderContextMenu>();
    menu.open = true;
    menu.client_order_id = Some("order-001".to_string());
    menu.venue = "MOCK".to_string();
    menu.screen_pos = Vec2::new(150.0, 250.0);
}

fn spawn_root(app: &mut App) -> Entity {
    // context_menu_visibility_system が ContextMenuRoot を必要とするので spawn する。
    app.world_mut().spawn((
        Node {
            display: Display::None,
            ..default()
        },
        ContextMenuRoot,
    )).id()
}

fn spawn_panel(app: &mut App) -> Entity {
    app.world_mut().spawn((
        Node {
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            ..default()
        },
        ContextMenuPanel,
    )).id()
}

#[test]
fn k16_order_context_menu_open_close() {
    // ── Case 1: open = true → visibility system が Display::Flex に同期 ──────────
    {
        let (mut app, _) = build_app();
        let root = spawn_root(&mut app);
        let _panel = spawn_panel(&mut app);

        open_menu(&mut app);
        app.update();

        let node = app.world().entity(root).get::<Node>().unwrap();
        assert_eq!(
            node.display,
            Display::Flex,
            "open=true must set ContextMenuRoot display to Flex"
        );
    }

    // ── Case 2: open = false → visibility system が Display::None に同期 ──────────
    {
        let (mut app, _) = build_app();
        let root = spawn_root(&mut app);
        let _panel = spawn_panel(&mut app);

        // まず open にしてから close。
        open_menu(&mut app);
        app.update();
        app.world_mut().resource_mut::<OrderContextMenu>().open = false;
        app.update();

        let node = app.world().entity(root).get::<Node>().unwrap();
        assert_eq!(
            node.display,
            Display::None,
            "open=false must set ContextMenuRoot display to None"
        );
    }

    // ── Case 3: backdrop click → menu が閉じる ──────────────────────────────────
    {
        let (mut app, _) = build_app();
        open_menu(&mut app);

        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            ContextMenuBackdrop,
        ));
        app.update();

        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "backdrop click must close the context menu"
        );
    }

    // ── Case 4: Escape (higher-priority modal なし) → menu が閉じる ──────────────
    {
        let (mut app, _) = build_app();
        open_menu(&mut app);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "Escape without higher-priority modal must close the context menu"
        );
    }

    // ── Case 5: Escape + SecretPrompt.active = Some → menu は閉じない ──────────
    // §3.10 優先度: SecretModal が Escape を消費する。
    {
        let (mut app, _) = build_app();
        open_menu(&mut app);
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r1".to_string(),
            venue: "MOCK".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(
            app.world().resource::<OrderContextMenu>().open,
            "Escape must not close context menu when SecretPrompt is active"
        );
    }

    // ── Case 6: Escape + OrderConfirm.pending = Some → menu は閉じない ──────────
    // OrderConfirm (確認モーダル) が開いているときは context menu の Escape は抑制される。
    {
        let (mut app, _) = build_app();
        open_menu(&mut app);
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.TSE".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        });

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(
            app.world().resource::<OrderContextMenu>().open,
            "Escape must not close context menu when OrderConfirm is pending"
        );
    }

    // ── Case 7: Escape + ModifyForm.open = true → menu は閉じない ───────────────
    {
        let (mut app, _) = build_app();
        open_menu(&mut app);
        app.world_mut().resource_mut::<ModifyForm>().open = true;

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(
            app.world().resource::<OrderContextMenu>().open,
            "Escape must not close context menu when ModifyForm is open"
        );
    }

    // ── Case 8: closed 状態での item click は no-op ──────────────────────────────
    {
        let (mut app, mut rx) = build_app();
        // open_menu を呼ばない → open = false のまま。

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "stale Cancel click on closed menu must not fire CancelOrder"
        );
        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "closed menu must stay closed after stale item click"
        );
    }

    // ── Case 9: item クリックでメニューが閉じる（CanCel アイテムの副作用確認）────
    {
        let (mut app, _rx) = build_app();
        open_menu(&mut app);

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();

        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "Cancel item click must close the context menu"
        );
    }

    // ── Case 10: Modify クリック後は menu が閉じ ModifyForm が開く ───────────────
    {
        let (mut app, mut rx) = build_app();
        open_menu(&mut app);

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Modify));
        app.update();

        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "Modify item click must close the context menu"
        );
        assert!(
            app.world().resource::<ModifyForm>().open,
            "Modify item click must open ModifyForm"
        );
        // Modify はコマンドを送らない。
        assert!(
            rx.try_recv().is_err(),
            "Modify item click must not fire a TransportCommand"
        );
    }
}
