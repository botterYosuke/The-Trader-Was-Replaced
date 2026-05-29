//! K14 reconcile_modal_dismiss_escape_priority — ReconcileModal は [確認した] ボタン / Escape で
//! unknown orders をクリアするが、Secret / OrderConfirm / ModifyModal が前面にあるときは Escape を
//! 譲ることを保証する（kind:ui）。
//!
//! **設計判断 (#46 B3, mechanism A)**: Escape による dismiss は `reconcile_modal_button_system`
//! から `modal_layer_esc_system`（汎用 modal-layer Esc）へ移管された。
//! reconcile (`reconcile_modal_reconcile_system`) が `ReconcilePrompt.unknown` ↔ `ModalLayer.stack`
//! を双方向同期する（`Local<bool> was_on_stack` で stack 側の pop を unknown.clear() へ逆反映）。
//! 上位 modal (SecretPrompt / OrderConfirm / ModifyForm) が開いていると esc system が Escape を
//! 譲る (`esc_yield_clear`)。ボタンクリックは従来どおり `reconcile_modal_button_system` が処理する。
//!
//! ordering: prod 同様に `reconcile.after(modal_layer_esc_system)` を張り、同フレームで
//! esc → reconcile が走って unknown が即クリアされることを保証する（warm-up update で
//! activate を stack に push 済みにしてから Escape フレームを回す）。

use bevy::prelude::*;

use backcast::trading::{
    ReconcilePrompt, ReconcileUnknownOrder, SecretPrompt, SecretPromptRequest,
};
use backcast::ui::component::modal_layer::{ModalLayer, modal_layer_esc_system};
use backcast::ui::modify_modal::{ModifyForm, ModifyModalRoot, modify_modal_reconcile_system};
use backcast::ui::order_panel::{
    confirm_modal_reconcile_system, ConfirmModalRoot, OrderConfirm, OrderDraft, OrderType, Side,
    TimeInForce,
};
use backcast::ui::reconcile_modal::{
    ReconcileDismissButton, ReconcileModalRoot, reconcile_modal_button_system,
    reconcile_modal_reconcile_system,
};

fn make_app() -> App {
    let mut app = App::new();
    app.init_resource::<ReconcilePrompt>();
    app.init_resource::<SecretPrompt>();
    app.init_resource::<OrderConfirm>();
    app.init_resource::<ModifyForm>();
    app.init_resource::<ModalLayer>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.world_mut().spawn(ReconcileModalRoot);
    app.world_mut().spawn(ConfirmModalRoot);
    app.world_mut().spawn(ModifyModalRoot);
    app.add_systems(
        Update,
        (
            reconcile_modal_button_system,
            modal_layer_esc_system,
            reconcile_modal_reconcile_system.after(modal_layer_esc_system),
            confirm_modal_reconcile_system.after(modal_layer_esc_system),
            modify_modal_reconcile_system.after(modal_layer_esc_system),
        ),
    );
    app
}

fn unknown_order(id: &str) -> ReconcileUnknownOrder {
    ReconcileUnknownOrder {
        client_order_id: id.to_string(),
        symbol: "1301.TSE".to_string(),
        status: "WORKING".to_string(),
    }
}

fn activate(app: &mut App) {
    app.world_mut().resource_mut::<ReconcilePrompt>().unknown = vec![unknown_order("c-k14")];
    app.update();
}

fn press_escape(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
}

#[test]
fn k14_reconcile_modal_dismiss_escape_priority() {
    // ── ケース 1: [確認した] ボタンで dismiss ──
    {
        let mut app = make_app();
        activate(&mut app);
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
        activate(&mut app);
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape without higher-priority modal must dismiss"
        );
    }

    // ── ケース 3: SecretPrompt 開 → Escape を譲る ──
    {
        let mut app = make_app();
        activate(&mut app);
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

    // ── ケース 4: OrderConfirm 開 → Escape は前面の確認モーダル (z=280) を閉じ reconcile (z=262) は残す ──
    // 5b 以降 OrderConfirm は esc_yield 入力ではなく stack entry (z=280)。warm-up update で
    // confirm を push 済みにしてから Escape を打つと、highest-z=confirm が dismiss され、
    // 背面の reconcile notice (z=262) は survive する（観測は reconcile survive で不変）。
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Sell,
            order_type: OrderType::Limit,
            qty: 10.0,
            price: Some(2500.0),
            tif: TimeInForce::Day,
        });
        app.update(); // warm-up: confirm を z=280 で stack に push。
        press_escape(&mut app);
        app.update();
        assert!(
            !app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape must dismiss the front OrderConfirm (z=280) — reconcile notice (z=262) must survive"
        );
    }

    // ── ケース 5: ModifyForm 開 → Escape は前面の訂正モーダル (z=270) を閉じ reconcile (z=262) は残す ──
    // 5c 以降 ModifyForm は esc_yield 入力ではなく stack entry (z=270)。warm-up update で
    // modify を push 済みにしてから Escape を打つと、highest-z=modify が dismiss され、
    // 背面の reconcile notice (z=262) は survive する（観測は reconcile survive で不変）。
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<ModifyForm>().open = true;
        app.update(); // warm-up: modify を z=270 で stack に push。
        press_escape(&mut app);
        app.update();
        assert!(
            !app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "Escape must dismiss the front ModifyModal (z=270) — reconcile notice (z=262) must survive"
        );
    }

    // ── ケース 6: unknown 空 → Pressed ボタンでも no-op ──
    {
        let mut app = make_app();
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();
        assert!(app.world().resource::<ReconcilePrompt>().unknown.is_empty());
    }

    // ── ケース 7: 複数 unknown order → dismiss で全件クリア ──
    {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReconcilePrompt>().unknown = vec![
            unknown_order("c1"),
            unknown_order("c2"),
            unknown_order("c3"),
        ];
        app.update();
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();
        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "dismiss must clear all unknown orders, not just the first"
        );
    }

    // ── ケース 8: 上位 modal が閉じれば 2 回目の Escape が通る ──
    {
        let mut app = make_app();
        activate(&mut app);
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
