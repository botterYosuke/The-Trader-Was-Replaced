//! K11 order_confirm_cancel_escape_priority — 発注確認モーダルは Cancel / Escape で閉じるが、
//! SecretModal が開いている場合は同じ Escape で二重に閉じないことを保証する（kind:ui）。
//!
//! テストでは `OrderConfirm.pending` と `SecretPrompt.active` の組み合わせを作り、Cancel/Escape 後の modal state を観測する。
//!
//! **設計判断 (#46 Slice B 5b, mechanism A)**: 確認モーダルの Escape dismiss は
//! `confirm_modal_button_system` 内の自前 Esc から `modal_layer_esc_system`（汎用 modal-layer Esc）
//! へ移管された。`confirm_modal_reconcile_system` が `OrderConfirm.pending` ↔ `ModalLayer.stack`
//! を双方向同期する（FORWARD で z=280 push、REVERSE/esc-pop で pending=None）。
//! 上位 modal (SecretPrompt / ModifyForm) が開いていると esc system が Escape を譲る
//! (`esc_yield_clear`)。Cancel/Confirm ボタンは従来どおり `confirm_modal_button_system` が処理する。
//!
//! ordering: prod 同様に `confirm_modal_reconcile_system.after(modal_layer_esc_system)` を張り、
//! 同フレームで esc → reconcile が走って pending が即クリアされることを保証する。pending を Some に
//! してから **1 回 app.update()** して confirm を stack に push 済みにしてから、別フレームで
//! Escape/Cancel を打つ（reconcile は `.after(esc)` なので、同フレームに pending=Some を入れて即
//! Escape すると esc が先に走り stack 空で no-op になり 1 フレームずれる）。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    OrderFeedback, SecretPrompt, SecretPromptRequest, TransportCommand, TransportCommandSender,
};
use backcast::ui::component::modal_layer::{ModalLayer, modal_layer_esc_system};
use backcast::ui::modify_modal::ModifyForm;
use backcast::ui::order_panel::{
    confirm_modal_button_system, confirm_modal_reconcile_system, ConfirmButton, ConfirmModalRoot,
    OrderConfirm, OrderDraft, OrderType, Side, TimeInForce,
};
use backcast::ui::secret_modal::{
    SecretInput, SecretModalRoot, secret_modal_reconcile_system,
};

fn build_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(OrderConfirm::default());
    app.insert_resource(SecretPrompt::default());
    app.insert_resource(OrderFeedback::default());
    app.init_resource::<ModifyForm>();
    app.init_resource::<SecretInput>();
    app.init_resource::<ModalLayer>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(TransportCommandSender { tx });

    app.world_mut().spawn(ConfirmModalRoot);
    app.world_mut().spawn(SecretModalRoot);
    app.add_systems(
        Update,
        (
            confirm_modal_button_system,
            modal_layer_esc_system,
            confirm_modal_reconcile_system.after(modal_layer_esc_system),
            secret_modal_reconcile_system.after(modal_layer_esc_system),
        ),
    );

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

/// pending=Some をセットしてから 1 回 update し、confirm を z=280 で stack に push 済みにする。
fn open_confirm(app: &mut App) {
    app.world_mut().resource_mut::<OrderConfirm>().pending = Some(sample_draft());
    app.update();
}

fn press_escape(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
}

#[test]
fn k11_order_confirm_cancel_escape_priority() {
    // ── Case 1: Cancel ボタン → pending クリア、コマンド送信なし ─────────────────
    {
        let (mut app, mut rx) = build_app();
        open_confirm(&mut app);

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
        open_confirm(&mut app);

        // Escape キーを注入。ButtonInput<KeyCode>::press は just_pressed を 1 フレーム有効にする。
        press_escape(&mut app);
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
    // SecretModal が Escape を消費するため esc system は yield し pending を維持する。
    {
        let (mut app, mut rx) = build_app();
        open_confirm(&mut app);
        // SecretPrompt を active にする。
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "req-1".to_string(),
            venue: "MOCK".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        app.update(); // warm-up: secret を z=300 で stack に push (confirm=z280 の前面)。

        press_escape(&mut app);
        app.update();

        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "Escape must dismiss the front SecretModal (z=300) — confirm modal (z=280) must survive"
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
        open_confirm(&mut app);
        // SecretPrompt active でまず Escape を打つ。
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "req-2".to_string(),
            venue: "MOCK".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        app.update(); // warm-up: secret を z=300 で stack に push (confirm=z280 の前面)。
        press_escape(&mut app);
        app.update();
        // confirm modal はまだ open (esc が secret z=300 を pop)。
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "first Escape (SecretPrompt active) must not close confirm modal"
        );

        // SecretPrompt を解除。ButtonInput をリセットして次フレームで再度 Escape。
        app.world_mut().resource_mut::<SecretPrompt>().active = None;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        press_escape(&mut app);
        app.update();
        // 今度は confirm modal が閉じる。
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "second Escape (SecretPrompt inactive) must close confirm modal"
        );
        assert!(rx.try_recv().is_err(), "Escape close must not fire command");
    }
}
