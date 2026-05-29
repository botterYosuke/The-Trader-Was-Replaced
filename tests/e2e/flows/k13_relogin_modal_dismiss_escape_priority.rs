//! K13 relogin_modal_dismiss_escape_priority — VenueLogoutDetected 後の再ログイン通知は Close / Escape で閉じるが、
//! Secret / OrderConfirm / ModifyModal が前面にあるときは Escape を譲ることを保証する（kind:ui）。
//!
//! **設計判断 (#46 B2-4 step 2+3, mechanism A)**: Escape による dismiss は
//! `relogin_modal_button_system` から `modal_layer_esc_system`（汎用 modal-layer Esc）へ移管された。
//! reconcile (`relogin_modal_reconcile_system`) が `ReloginPrompt.active` ↔ `ModalLayer.stack` を
//! 双方向同期する（`Local<bool> was_on_stack` で stack 側の pop を prompt.active=None へ逆反映）。
//! 上位 modal (SecretPrompt / OrderConfirm / ModifyForm) が開いていると esc system が Escape を
//! 譲る (`esc_yield_clear`)。ボタンクリックは従来どおり `relogin_modal_button_system` が処理する。
//!
//! ordering: prod 同様に `reconcile.after(modal_layer_esc_system)` を張り、同フレームで
//! esc → reconcile が走って prompt.active が即クリアされることを保証する（warm-up update で
//! activate を stack に push 済みにしてから Escape フレームを回す）。

use bevy::prelude::*;

use backcast::trading::{ReloginPrompt, SecretPrompt, SecretPromptRequest};
use backcast::ui::component::modal_layer::{ModalLayer, modal_layer_esc_system};
use backcast::ui::modify_modal::{ModifyForm, ModifyModalRoot, modify_modal_reconcile_system};
use backcast::ui::order_panel::{
    confirm_modal_reconcile_system, ConfirmModalRoot, OrderConfirm, OrderDraft, OrderType, Side,
    TimeInForce,
};
use backcast::ui::relogin_modal::{
    ReloginDismissButton, ReloginModalRoot, relogin_modal_button_system,
    relogin_modal_reconcile_system,
};
use backcast::ui::secret_modal::{
    SecretInput, SecretModalRoot, secret_modal_reconcile_system,
};

fn make_app() -> App {
    let mut app = App::new();
    app.init_resource::<ReloginPrompt>();
    app.init_resource::<SecretPrompt>();
    app.init_resource::<OrderConfirm>();
    app.init_resource::<ModifyForm>();
    app.init_resource::<SecretInput>();
    app.init_resource::<ModalLayer>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.world_mut().spawn(ReloginModalRoot);
    app.world_mut().spawn(ConfirmModalRoot);
    app.world_mut().spawn(ModifyModalRoot);
    app.world_mut().spawn(SecretModalRoot);
    app.add_systems(
        Update,
        (
            relogin_modal_button_system,
            modal_layer_esc_system,
            relogin_modal_reconcile_system.after(modal_layer_esc_system),
            confirm_modal_reconcile_system.after(modal_layer_esc_system),
            modify_modal_reconcile_system.after(modal_layer_esc_system),
            secret_modal_reconcile_system.after(modal_layer_esc_system),
        ),
    );
    app
}

fn activate(app: &mut App) {
    app.world_mut().resource_mut::<ReloginPrompt>().active = Some("TACHIBANA".to_string());
    app.update();
}

fn press_escape(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
}

#[test]
fn k13_relogin_modal_dismiss_escape_priority() {
    // ── ケース 1: [閉じる] ボタンで dismiss ──
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReloginDismissButton));
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_none(),
            "dismiss button must clear relogin prompt"
        );
    }

    // ── ケース 2: 上位 modal なし → Escape で dismiss ──
    {
        let mut app = make_app();
        activate(&mut app);
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_none(),
            "Escape without higher-priority modal must dismiss"
        );
    }

    // ── ケース 3: SecretPrompt 開 → Escape を譲る ──
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r-k13".to_string(),
            venue: "TACHIBANA".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        app.update(); // warm-up: secret を z=300 で stack に push。
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "Escape must dismiss the front SecretModal (z=300) — relogin notice (z=260) must survive"
        );
    }

    // ── ケース 4: OrderConfirm 開 → Escape は前面の確認モーダル (z=280) を閉じ relogin (z=260) は残す ──
    // 5b 以降 OrderConfirm は esc_yield 入力ではなく stack entry (z=280)。warm-up update で
    // confirm を push 済みにしてから Escape を打つと、highest-z=confirm が dismiss され、
    // 背面の relogin notice (z=260) は survive する（観測は relogin survive で不変）。
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        });
        app.update(); // warm-up: confirm を z=280 で stack に push。
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "Escape must dismiss the front OrderConfirm (z=280) — relogin notice (z=260) must survive"
        );
    }

    // ── ケース 5: ModifyForm 開 → Escape は前面の訂正モーダル (z=270) を閉じ relogin (z=260) は残す ──
    // 5c 以降 ModifyForm は esc_yield 入力ではなく stack entry (z=270)。warm-up update で
    // modify を push 済みにしてから Escape を打つと、highest-z=modify が dismiss され、
    // 背面の relogin notice (z=260) は survive する（観測は relogin survive で不変）。
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<ModifyForm>().open = true;
        app.update(); // warm-up: modify を z=270 で stack に push。
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "Escape must dismiss the front ModifyModal (z=270) — relogin notice (z=260) must survive"
        );
    }

    // ── ケース 6: prompt 閉 → Pressed ボタンでも no-op ──
    {
        let mut app = make_app();
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReloginDismissButton));
        app.update();
        assert!(app.world().resource::<ReloginPrompt>().active.is_none());
    }

    // ── ケース 7: 上位 modal が閉じれば 2 回目の Escape が通る ──
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r-k13-b".to_string(),
            venue: "TACHIBANA".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        app.update(); // warm-up: secret を z=300 で stack に push。
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "first Escape must be absorbed by SecretModal"
        );

        app.world_mut().resource_mut::<SecretPrompt>().active = None;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_none(),
            "second Escape (no higher-priority modal) must dismiss the relogin notice"
        );
    }
}
