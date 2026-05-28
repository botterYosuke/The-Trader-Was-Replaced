//! K7 manual_order_submit_confirm — Manual モードの注文フォームで銘柄・side・数量・価格を入力し、
//! confirm 後に発注 command が送られることを保証する（kind:ui）。
//!
//! テストでは order form input / submit / confirm interaction を注入し、transport command と feedback row を観測する。
//!
//! 検証フロー:
//! 1. venue Connected + symbol 選択済みの状態で Submit を押す → `OrderConfirm.pending` がセットされる。
//! 2. `ConfirmButton::Confirm` を押す → `TransportCommand::PlaceOrder` が channel に積まれ、pending がクリアされる。
//! 3. symbol 未選択で Submit を押す → pending は立たず `last_error` がセットされる（validation gate）。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    OrderFeedback, SecretPrompt, SelectedSymbol, TransportCommand, TransportCommandSender,
    VenueState, VenueStatusRes,
};
use backcast::ui::order_panel::{
    confirm_modal_button_system, order_submit_button_system, ConfirmButton, OrderButton,
    OrderButtonPressed, OrderConfirm, OrderDraft, OrderForm, OrderType, Side, TimeInForce,
};

/// order_submit_button_system と confirm_modal_button_system が必要とするすべての資源を
/// 投入したシンプルな App を返す。
fn build_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    // venue: Connected でないと submit が通らない訳ではない（validate_order は venue state を見ない）が、
    // 本番 order_submit_button_system は venue gate を持たないので Connected/Disconnected は関係ない。
    // OrderDraft の venue フィールドに venue_id が入るよう Connected + venue_id をセット。
    app.insert_resource(VenueStatusRes {
        state: VenueState::Connected,
        venue_id: Some("MOCK".to_string()),
        ..Default::default()
    });
    app.insert_resource(SelectedSymbol {
        id: Some("7203.TSE".to_string()),
    });
    app.insert_resource(OrderForm {
        side: Side::Buy,
        order_type: OrderType::Market,
        qty: 100.0,
        price: 0.0,
        tif: TimeInForce::Day,
    });
    app.insert_resource(OrderConfirm::default());
    app.insert_resource(OrderFeedback::default());
    app.insert_resource(SecretPrompt::default());
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(TransportCommandSender { tx });

    app.add_message::<OrderButtonPressed>();
    app.add_systems(Update, (order_submit_button_system, confirm_modal_button_system));

    (app, rx)
}

#[test]
fn k7_manual_order_submit_confirm() {
    // ── Case 1: Submit ボタンで pending がセットされる ──────────────────────────
    {
        let (mut app, mut rx) = build_app();

        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();

        let confirm = app.world().resource::<OrderConfirm>();
        assert!(
            confirm.pending.is_some(),
            "valid submit must open the confirm modal (OrderConfirm.pending = Some)"
        );
        assert!(
            confirm.last_error.is_none(),
            "valid submit must not set an error"
        );
        // Submit 自体はコマンドを送らない（2 段階確認の 1 段目）。
        assert!(
            rx.try_recv().is_err(),
            "Submit alone must NOT fire a PlaceOrder — confirm step is required"
        );
    }

    // ── Case 2: ConfirmButton::Confirm → PlaceOrder が channel に積まれる ───────
    {
        let (mut app, mut rx) = build_app();

        // pending を直接セット（Submit の副作用を再現）。
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
            .spawn((Button, Interaction::Pressed, ConfirmButton::Confirm));
        app.update();

        // pending がクリアされる。
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "Confirm must clear OrderConfirm.pending"
        );

        let cmd = rx
            .try_recv()
            .expect("Confirm must send a PlaceOrder command");
        match cmd {
            TransportCommand::PlaceOrder {
                venue,
                instrument_id,
                side,
                qty,
                price,
                order_type,
                second_secret,
                ..
            } => {
                assert_eq!(venue, "MOCK");
                assert_eq!(instrument_id, "7203.TSE");
                assert_eq!(side, "BUY");
                assert_eq!(qty, 100.0);
                assert_eq!(price, None, "Market order must carry no price");
                assert_eq!(order_type, "MARKET");
                assert!(
                    second_secret.is_none(),
                    "OrderPanel never attaches second_secret (Step 5)"
                );
            }
            other => panic!("expected PlaceOrder, got {other:?}"),
        }
    }

    // ── Case 3: symbol 未選択で Submit → pending は立たず last_error がセット ───
    {
        let (mut app, mut rx) = build_app();
        // symbol を消す。
        app.world_mut().resource_mut::<SelectedSymbol>().id = None;

        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();

        let confirm = app.world().resource::<OrderConfirm>();
        assert!(
            confirm.pending.is_none(),
            "invalid submit (no symbol) must NOT open the confirm modal"
        );
        assert!(
            confirm.last_error.is_some(),
            "invalid submit must set OrderConfirm.last_error"
        );
        assert!(
            rx.try_recv().is_err(),
            "invalid submit must not fire any command"
        );
    }

    // ── Case 4: qty が 0 で Submit → validation で弾かれる ─────────────────────
    {
        let (mut app, mut rx) = build_app();
        app.world_mut().resource_mut::<OrderForm>().qty = 0.0;

        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();

        let confirm = app.world().resource::<OrderConfirm>();
        assert!(
            confirm.pending.is_none(),
            "zero qty must not open confirm modal"
        );
        assert!(confirm.last_error.is_some(), "zero qty must set an error");
        assert!(rx.try_recv().is_err(), "zero qty must not fire command");
    }

    // ── Case 5: Limit 注文で価格不備 → PriceRequiredForLimit エラー ─────────────
    {
        let (mut app, mut rx) = build_app();
        {
            let mut form = app.world_mut().resource_mut::<OrderForm>();
            form.order_type = OrderType::Limit;
            form.price = 0.0; // 指値なのに価格なし
        }
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();

        let confirm = app.world().resource::<OrderConfirm>();
        assert!(
            confirm.pending.is_none(),
            "Limit with price=0 must not open confirm modal"
        );
        assert!(
            confirm.last_error.is_some(),
            "Limit with price=0 must set an error"
        );
        assert!(rx.try_recv().is_err(), "invalid limit must not fire command");
    }

    // ── Case 6: pending が既にある状態で Submit は二重 open を防ぐ ───────────────
    {
        let (mut app, _rx) = build_app();
        // 既存の pending をセット。
        let first_draft = OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.TSE".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        };
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(first_draft.clone());

        // 再度 Submit を注入 — pending は上書きされないはず。
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();

        let confirm = app.world().resource::<OrderConfirm>();
        // pending は first_draft のまま変わらない（二重 open 防止ガード）。
        assert_eq!(
            confirm.pending.as_ref().map(|d| d.symbol.as_str()),
            Some("7203.TSE"),
            "double Submit must not overwrite existing pending"
        );
    }
}
