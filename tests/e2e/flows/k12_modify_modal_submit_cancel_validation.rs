//! K12 modify_modal_submit_cancel_validation — 訂正モーダルで qty/price を編集し、確認チェック後だけ
//! ModifyOrder command が送られ、Cancel / Escape では送られないことを保証する（kind:ui）。
//!
//! **設計判断**: `modify_modal_button_system` は `form.open == false` のとき early return する。
//! keyboard drain (`modify_modal_input_system`) も同様に early return する。
//! `parse_buf` が > 0 の有限値のみ `Some` にするため、空欄・"abc"・"0" は validation に弾かれる。
//! kabu venue は `requires_kabu_ack()` が true → ack なしでは can_confirm() が false。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{OrderFeedback, SecretPrompt, TransportCommand, TransportCommandSender};
use backcast::ui::component::modal_layer::{ModalLayer, modal_layer_esc_system};
use backcast::ui::modify_modal::{
    ModifyButton, ModifyForm, ModifyFocus, ModifyModalRoot, modify_modal_button_system,
    modify_modal_input_system, modify_modal_reconcile_system,
};

// ── ヘルパー ──────────────────────────────────────────────────────────────────

fn make_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = App::new();
    app.init_resource::<ModifyForm>();
    app.init_resource::<OrderFeedback>();
    app.init_resource::<ModalLayer>();
    app.init_resource::<SecretPrompt>();
    app.insert_resource(TransportCommandSender { tx });
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.add_message::<KeyboardInput>();
    app.world_mut().spawn(ModifyModalRoot);
    app.add_systems(
        Update,
        (
            modify_modal_button_system,
            modify_modal_input_system,
            modal_layer_esc_system,
            modify_modal_reconcile_system.after(modal_layer_esc_system),
        ),
    );
    (app, rx)
}

fn open_form(app: &mut App, venue: &str) {
    let mut f = app.world_mut().resource_mut::<ModifyForm>();
    f.open = true;
    f.client_order_id = "cid-k12".to_string();
    f.venue = venue.to_string();
    f.new_qty_buf.clear();
    f.new_price_buf.clear();
    f.ack_kabu = false;
    f.focus = ModifyFocus::Qty;
}

#[test]
fn k12_modify_modal_submit_cancel_validation() {
    // ── ケース 1: qty + price を入力して Confirm → ModifyOrder 送信・モーダル閉じる ──
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "MOCK");
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            f.new_qty_buf = "200".to_string();
            f.new_price_buf = "2600".to_string();
        }
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();

        let cmd = rx.try_recv().expect("Confirm must fire ModifyOrder");
        match cmd {
            TransportCommand::ModifyOrder {
                venue,
                client_order_id,
                new_qty,
                new_price,
                second_secret,
            } => {
                assert_eq!(venue, "MOCK");
                assert_eq!(client_order_id, "cid-k12");
                assert_eq!(new_qty, Some(200.0));
                assert_eq!(new_price, Some(2600.0));
                assert!(second_secret.is_none(), "Step 4 always sends None");
            }
            other => panic!("expected ModifyOrder, got {other:?}"),
        }
        assert!(
            !app.world().resource::<ModifyForm>().open,
            "Confirm must close the modal"
        );
    }

    // ── ケース 2: 空欄で Confirm → command なし・feedback に案内・モーダル開いたまま ──
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "MOCK");
        // both bufs empty

        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();

        assert!(rx.try_recv().is_err(), "empty modify must not fire a command");
        assert!(
            app.world().resource::<ModifyForm>().open,
            "modal stays open when validation fails"
        );
        assert!(
            app.world().resource::<OrderFeedback>().message.is_some(),
            "feedback must explain what to enter"
        );
    }

    // ── ケース 3: Cancel ボタン → command なし・モーダル閉じる ──
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "MOCK");
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            f.new_qty_buf = "300".to_string();
        }
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Cancel));
        app.update();

        assert!(rx.try_recv().is_err(), "Cancel must not fire a command");
        assert!(!app.world().resource::<ModifyForm>().open);
    }

    // ── ケース 4: Escape キーで cancel — modal-layer 経路 (z=270 stack entry) ──
    // 5c 以降 ModifyForm は esc-drain で閉じず、reconcile が open=true を z=270 で stack に
    // push する。warm-up update で push 済みにしてから Escape を打つと、
    // highest-z=modify(270) が dismiss され REVERSE で form.close() が走る。
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "MOCK");
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            f.new_price_buf = "3000".to_string();
        }
        app.update(); // warm-up: modify を z=270 で stack に push。
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(rx.try_recv().is_err(), "Escape must not fire a command");
        assert!(
            !app.world().resource::<ModifyForm>().open,
            "Escape must close the modal"
        );
    }

    // ── ケース 5: Enter キーで Confirm — keyboard drain 経由 ──
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "MOCK");
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            f.new_qty_buf = "100".to_string();
        }
        app.world_mut()
            .resource_mut::<Messages<KeyboardInput>>()
            .write(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            text: None,
            });
        app.update();

        let cmd = rx.try_recv().expect("Enter must fire ModifyOrder");
        assert!(matches!(cmd, TransportCommand::ModifyOrder { .. }));
        assert!(!app.world().resource::<ModifyForm>().open);
    }

    // ── ケース 6: kabu venue — ack なしでは Confirm blocked、ack 後は OK ──
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "kabu");
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            f.new_qty_buf = "50".to_string();
            // ack_kabu は false のまま
        }
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();

        assert!(rx.try_recv().is_err(), "kabu unack must block Confirm");
        assert!(
            app.world().resource::<ModifyForm>().open,
            "modal stays open"
        );

        // ack toggle → 再 Confirm
        app.world_mut().resource_mut::<ModifyForm>().ack_kabu = true;
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();

        let cmd = rx.try_recv().expect("ack + Confirm must fire ModifyOrder");
        assert!(matches!(cmd, TransportCommand::ModifyOrder { .. }));
    }

    // ── ケース 7: validate — "abc" / "0" / "-1" は None 扱いで blocked ──
    {
        let (mut app, mut rx) = make_app();
        open_form(&mut app, "MOCK");
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            // parse_buf rejects non-positive/invalid strings
            f.new_qty_buf = "abc".to_string();
            f.new_price_buf = "0".to_string();
        }
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "invalid qty/price must be treated as None (no change)"
        );
        assert!(app.world().resource::<ModifyForm>().open);
    }
}
