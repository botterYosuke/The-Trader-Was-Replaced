//! K11 order_confirm_cancel_escape_priority — 発注確認モーダルは Cancel / Escape で閉じるが、
//! SecretModal が開いている場合は同じ Escape で二重に閉じないことを保証する（kind:ui）。
//!
//! テストでは `OrderConfirm.pending` と `SecretPrompt.active` の組み合わせを作り、Cancel/Escape 後の modal state を観測する。
//!
//! 優先度ロジック (§3.10 / confirm_modal_button_system コメント "item 9 + item 7"):
//! - `SecretPrompt.active.is_some()` → Escape は SecretModal が消費するとみなし、
//!   confirm modal は **閉じない** (`pending` を維持する)。
//! - `SecretPrompt.active.is_none()` → Escape で `pending` をクリアしコマンドを送らない。
//! - Cancel ボタン → `pending` をクリアしコマンドを送らない（Escape と同じ動作）。
//! - Confirm ボタン → `pending` をクリアして PlaceOrder を送る（K7 の主ケース）。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    OrderFeedback, SecretPrompt, SecretPromptRequest, TransportCommand, TransportCommandSender,
};
use backcast::ui::order_panel::{
    confirm_modal_button_system, ConfirmButton, OrderConfirm, OrderDraft, OrderType,
    Side, TimeInForce,
};

fn build_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(OrderConfirm::default());
    app.insert_resource(SecretPrompt::default());
    app.insert_resource(OrderFeedback::default());
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(TransportCommandSender { tx });

    app.add_systems(Update, confirm_modal_button_system);

    (app, rx)
}

fn sample_draft() -> OrderDraft {
    OrderDraft {
        venue: "MOCK".to_string(),
        symbol: "7203.TSE".to_string(),
        side: Side::Buy,
        order_type: OrderType::Market,
        qty: 100.0,
        price: None,
        tif: TimeInForce::Day,
    }
}

#[test]
fn k11_order_confirm_cancel_escape_priority() {
    // ── Case 1: Cancel ボタン → pending クリア、コマンド送信なし ─────────────────
    {
        let (mut app, mut rx) = build_app();
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(sample_draft());

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ConfirmButton::Cancel));
        app.update();

        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "Cancel button must clear pending"
        );
        assert!(
            rx.try_recv().is_err(),
            "Cancel button must not fire any TransportCommand"
        );
    }

    // ── Case 2: Escape (SecretPrompt なし) → pending クリア、コマンド送信なし ────
    {
        let (mut app, mut rx) = build_app();
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(sample_draft());

        // Escape キーを注入。ButtonInput<KeyCode>::press は just_pressed を 1 フレーム有効にする。
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "Escape without SecretPrompt must clear pending"
        );
        assert!(
            rx.try_recv().is_err(),
            "Escape cancel must not fire any TransportCommand"
        );
    }

    // ── Case 3: Escape + SecretPrompt.active = Some → confirm modal は閉じない ──
    // §3.10 「one keystroke must not close both」優先度ルール。
    // SecretModal が Escape を消費するため confirm_modal_button_system は pending を維持する。
    {
        let (mut app, mut rx) = build_app();
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(sample_draft());
        // SecretPrompt を active にする。
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "req-1".to_string(),
            venue: "MOCK".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "Escape must NOT close confirm modal when SecretPrompt is active (higher priority)"
        );
        assert!(
            rx.try_recv().is_err(),
            "no command must be fired when Escape is suppressed by SecretPrompt"
        );
    }

    // ── Case 4: pending = None のとき Confirm ボタンは no-op (安全ガード) ─────────
    // 最も安全性が重要なボタン（本番資金の PlaceOrder）の二重発射防止。
    {
        let (mut app, mut rx) = build_app();
        // pending は None のまま。

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ConfirmButton::Confirm));
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "Confirm with no pending must NOT fire PlaceOrder"
        );
    }

    // ── Case 5: pending = None のとき Cancel ボタンは no-op ─────────────────────
    // confirm_modal_button_system は pending.is_none() で早期リターンするため影響なし。
    {
        let (mut app, mut rx) = build_app();

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ConfirmButton::Cancel));
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "Cancel with no pending must not fire any command"
        );
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "pending must stay None after Cancel with no pending"
        );
    }

    // ── Case 6: Escape 後に SecretPrompt を解除 → 次フレームの Escape は閉じる ─────
    // 優先度が動的に変わることを確認する。
    {
        let (mut app, mut rx) = build_app();
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(sample_draft());
        // SecretPrompt active でまず Escape を打つ。
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "req-2".to_string(),
            venue: "MOCK".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        // confirm modal はまだ open。
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "first Escape (SecretPrompt active) must not close confirm modal"
        );

        // SecretPrompt を解除。ButtonInput をリセットして次フレームで再度 Escape。
        app.world_mut().resource_mut::<SecretPrompt>().active = None;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        // 今度は confirm modal が閉じる。
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "second Escape (SecretPrompt inactive) must close confirm modal"
        );
        assert!(rx.try_recv().is_err(), "Escape close must not fire command");
    }
}
