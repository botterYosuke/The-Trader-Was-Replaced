//! K13 relogin_modal_dismiss_escape_priority — VenueLogoutDetected 後の再ログイン通知は Close / Escape で閉じるが、
//! Secret / OrderConfirm / ModifyModal が前面にあるときは Escape を譲ることを保証する（kind:ui）。
//!
//! **設計判断**: `relogin_modal_button_system` は `ButtonInput<KeyCode>` を読む (keyboard drain ではない)。
//! 上位 modal (SecretPrompt / OrderConfirm / ModifyForm) が開いている場合、`higher_priority_open`
//! フラグが true になり Escape は無視される。ボタンクリックは常に有効 (higher_priority は関係なし)。
//! テストでは `ButtonInput::<KeyCode>::press()` で Escape を注入する。

use bevy::prelude::*;

use backcast::trading::{ReloginPrompt, SecretPrompt, SecretPromptRequest};
use backcast::ui::modify_modal::ModifyForm;
use backcast::ui::order_panel::{OrderConfirm, OrderDraft, OrderType, Side, TimeInForce};
use backcast::ui::relogin_modal::{ReloginDismissButton, relogin_modal_button_system};

// ── ヘルパー ──────────────────────────────────────────────────────────────────

fn make_app() -> App {
    let mut app = App::new();
    app.init_resource::<ReloginPrompt>();
    app.init_resource::<SecretPrompt>();
    app.init_resource::<OrderConfirm>();
    app.init_resource::<ModifyForm>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.add_systems(Update, relogin_modal_button_system);
    app
}

fn activate(app: &mut App) {
    app.world_mut().resource_mut::<ReloginPrompt>().active = Some("TACHIBANA".to_string());
}

fn press_escape(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
}

#[test]
fn k13_relogin_modal_dismiss_escape_priority() {
    // ── ケース 1: [閉じる] ボタンで dismiss — prompt が None になる ──
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

    // ── ケース 3: SecretPrompt が開いている → Escape はレログイン通知を閉じない ──
    // §3.10: 一つの Escape で two modals が閉じると one-shot request_id を浪費する。
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r-k13".to_string(),
            venue: "TACHIBANA".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        press_escape(&mut app);
        app.update();

        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "Escape must yield to open SecretModal — relogin notice must survive"
        );
    }

    // ── ケース 4: OrderConfirm が開いている → Escape はレログイン通知を閉じない ──
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
        press_escape(&mut app);
        app.update();

        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "Escape must yield to open OrderConfirm — relogin notice must survive"
        );
    }

    // ── ケース 5: ModifyForm が開いている → Escape はレログイン通知を閉じない ──
    {
        let mut app = make_app();
        activate(&mut app);
        app.world_mut().resource_mut::<ModifyForm>().open = true;
        press_escape(&mut app);
        app.update();

        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "Escape must yield to open ModifyModal — relogin notice must survive"
        );
    }

    // ── ケース 6: prompt が閉じているとき — Pressed ボタンがあっても no-op ──
    // early return (prompt.active.is_none()) を確認する。
    {
        let mut app = make_app();
        // prompt は None のまま

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReloginDismissButton));
        app.update();

        // パニックせず active が None のままであること
        assert!(app.world().resource::<ReloginPrompt>().active.is_none());
    }

    // ── ケース 7: 上位 modal が閉じれば Escape が通る ──
    // SecretPrompt が閉じた後にもう一度 Escape を押すと通知が消える。
    {
        let mut app = make_app();
        activate(&mut app);

        // 最初は SecretPrompt open → Escape を無視
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r-k13-b".to_string(),
            venue: "TACHIBANA".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        press_escape(&mut app);
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_some(),
            "first Escape must be absorbed by SecretModal"
        );

        // SecretPrompt が閉じた後に次フレームで Escape — 今度は relogin が閉じる
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
            app.world().resource::<ReloginPrompt>().active.is_none(),
            "second Escape (no higher-priority modal) must dismiss the relogin notice"
        );
    }
}
