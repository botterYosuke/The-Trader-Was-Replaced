//! K14 reconcile_modal_dismiss_escape_priority — ReconcileModal は確認ボタン / Escape で unknown orders をクリアするが、
//! Secret / OrderConfirm / ModifyModal が前面にあるときは Escape を譲ることを保証する（kind:ui）。
//!
//! **設計判断**: `reconcile_modal_button_system` は `ButtonInput<KeyCode>` を読む (keyboard drain ではない)。
//! 上位 modal (SecretPrompt / OrderConfirm / ModifyForm) が開いている場合、`higher_priority_open`
//! フラグが true になり Escape は無視される。ボタンクリックは常に有効。
//! `prompt.unknown.is_empty()` が true のときは early return する (no-op)。

use bevy::prelude::*;

use backcast::trading::{
    ReconcilePrompt, ReconcileUnknownOrder, SecretPrompt, SecretPromptRequest,
};
use backcast::ui::modify_modal::ModifyForm;
use backcast::ui::order_panel::{OrderConfirm, OrderDraft, OrderType, Side, TimeInForce};
use backcast::ui::reconcile_modal::{ReconcileDismissButton, reconcile_modal_button_system};

// ── ヘルパー ──────────────────────────────────────────────────────────────────

fn make_app() -> App {
    let mut app = App::new();
    app.init_resource::<ReconcilePrompt>();
    app.init_resource::<SecretPrompt>();
    app.init_resource::<OrderConfirm>();
    app.init_resource::<ModifyForm>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.add_systems(Update, reconcile_modal_button_system);
    app
}

fn unknown_order(id: &str) -> ReconcileUnknownOrder {
    ReconcileUnknownOrder {
        client_order_id: id.to_string(),
        symbol: "1301.TSE".to_string(),
        status: "WORKING".to_string(),
    }
}

fn seed_unknown(app: &mut App) {
    app.world_mut().resource_mut::<ReconcilePrompt>().unknown = vec![unknown_order("c-k14")];
}

fn press_escape(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
}

#[test]
fn k14_reconcile_modal_dismiss_escape_priority() {
    // ── ケース 1: [確認した] ボタンで dismiss — unknown が空になる ──
    {
        let mut app = make_app();
        seed_unknown(&mut app);

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();

        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "confirm button must clear reconcile prompt"
        );
    }

    // ── ケース 2: 上位 modal なし → Escape で dismiss ──
    {
        let mut app = make_app();
        seed_unknown(&mut app);
        press_escape(&mut app);
        app.update();

        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape without higher-priority modal must dismiss"
        );
    }

    // ── ケース 3: SecretPrompt が開いている → Escape は reconcile 通知を閉じない ──
    {
        let mut app = make_app();
        seed_unknown(&mut app);
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r-k14".to_string(),
            venue: "TACHIBANA".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        press_escape(&mut app);
        app.update();

        assert!(
            !app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape must yield to open SecretModal — reconcile notice must survive"
        );
    }

    // ── ケース 4: OrderConfirm が開いている → Escape は reconcile 通知を閉じない ──
    {
        let mut app = make_app();
        seed_unknown(&mut app);
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Sell,
            order_type: OrderType::Limit,
            qty: 10.0,
            price: Some(2500.0),
            tif: TimeInForce::Day,
        });
        press_escape(&mut app);
        app.update();

        assert!(
            !app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape must yield to open OrderConfirm — reconcile notice must survive"
        );
    }

    // ── ケース 5: ModifyForm が開いている → Escape は reconcile 通知を閉じない ──
    {
        let mut app = make_app();
        seed_unknown(&mut app);
        app.world_mut().resource_mut::<ModifyForm>().open = true;
        press_escape(&mut app);
        app.update();

        assert!(
            !app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape must yield to open ModifyModal — reconcile notice must survive"
        );
    }

    // ── ケース 6: unknown が空のとき — Pressed ボタンがあっても no-op (early return) ──
    {
        let mut app = make_app();
        // unknown は空 (デフォルト)

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();

        assert!(app.world().resource::<ReconcilePrompt>().unknown.is_empty());
    }

    // ── ケース 7: 複数の unknown order — dismiss で全件クリアされる ──
    {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReconcilePrompt>().unknown = vec![
            unknown_order("c1"),
            unknown_order("c2"),
            unknown_order("c3"),
        ];

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();

        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "dismiss must clear all unknown orders, not just the first"
        );
    }

    // ── ケース 8: 上位 modal が閉じれば Escape が通る (k13 と対称) ──
    {
        let mut app = make_app();
        seed_unknown(&mut app);

        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r-k14-b".to_string(),
            venue: "TACHIBANA".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        press_escape(&mut app);
        app.update();
        assert!(
            !app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "first Escape absorbed by SecretModal"
        );

        app.world_mut().resource_mut::<SecretPrompt>().active = None;
        // bare App には input plugin が無いため ButtonInput が自動で clear されない。
        // press() は既に pressed のキーには no-op (just_pressed が再生成されない) ので、
        // reset_all() で一度すべて release してから fresh press し直す必要がある。
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "second Escape (no higher-priority modal) must dismiss reconcile notice"
        );
    }
}
