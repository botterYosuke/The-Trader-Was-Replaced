//! K8 secret_modal_submit_retry — SecretRequired 後の第二暗証番号モーダルで入力・送信でき、
//! 提出失敗時は retry 可能な error を表示し、Escape で意図しない永続化をしないことを保証する（kind:ui）。
//!
//! **入力方法**: `SecretInput.push_char` は private なので外部テストは
//! `secret_modal_input_system` が drain する `Messages<KeyboardInput>` にキャラクターを
//! 送って buffer を埋める (production と同じ経路)。
//!
//! **submit/retry**: `do_submit` は `prompt.error` を None にクリアしてから送信するため、
//! error が残っていても再 submit は成功する。
//! **cancel**: `do_cancel` は `prompt.close()` 経由で `active` + `error` 両方を消す。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    OrderFeedback, SecretPrompt, SecretPromptRequest, TransportCommand, TransportCommandSender,
};
use backcast::ui::component::modal_layer::{ModalLayer, modal_layer_esc_system};
use backcast::ui::secret_modal::{
    SecretButton, SecretInput, SecretModalRoot, secret_modal_button_system,
    secret_modal_input_system, secret_modal_reconcile_system,
};

// ── ヘルパー ──────────────────────────────────────────────────────────────────

fn make_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = App::new();
    app.init_resource::<SecretInput>();
    app.init_resource::<SecretPrompt>();
    app.init_resource::<OrderFeedback>();
    app.init_resource::<ModalLayer>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(TransportCommandSender { tx });
    // keyboard drain が要求する Messages<KeyboardInput>
    app.add_message::<KeyboardInput>();
    app.world_mut().spawn(SecretModalRoot);
    // input_system を先に、button_system を後に (同フレーム内の順序は任意でも OK だが明示する)
    app.add_systems(
        Update,
        (
            secret_modal_input_system,
            secret_modal_button_system,
            modal_layer_esc_system,
            secret_modal_reconcile_system
                .after(modal_layer_esc_system)
                .after(secret_modal_input_system),
        ),
    );
    (app, rx)
}

fn activate(app: &mut App, request_id: &str) {
    app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
        request_id: request_id.to_string(),
        venue: "tachibana".to_string(),
        kind: "second_secret".to_string(),
        purpose: "new_order".to_string(),
    });
}

/// `Messages<KeyboardInput>` に文字列を送ることで production の keyboard drain 経路を通して
/// SecretInput バッファを埋める。push_char は private なのでこの経路しかない。
/// 呼び出し後に `app.update()` が必要 (input_system がその時点で drain する)。
fn type_into_modal(app: &mut App, s: &str) {
    for c in s.chars() {
        let cs = c.to_string();
        app.world_mut()
            .resource_mut::<Messages<KeyboardInput>>()
            .write(KeyboardInput {
                // key_code は secret_modal_input_system が参照しない。logical_key のみ使用。
                key_code: KeyCode::F35,
                logical_key: Key::Character(cs.as_str().into()),
                state: ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            text: None,
            });
    }
}

fn send_enter(app: &mut App) {
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
}

#[test]
fn k8_secret_modal_submit_retry() {
    // ── ケース 1: 正常 submit (Submit ボタン) — command が送られ buffer と prompt が閉じる ──
    {
        let (mut app, mut rx) = make_app();
        activate(&mut app, "req-k8-a");

        // "1234" を keyboard drain 経由で入力
        type_into_modal(&mut app, "1234");
        app.update(); // input_system が drain → buffer = "1234"

        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update(); // button_system が Pressed を処理

        let cmd = rx.try_recv().expect("submit must fire SubmitSecret");
        match cmd {
            TransportCommand::SubmitSecret { request_id, secret } => {
                assert_eq!(request_id, "req-k8-a");
                assert_eq!(secret.expose(), "1234");
            }
            other => panic!("expected SubmitSecret, got {other:?}"),
        }
        assert!(app.world().resource::<SecretInput>().is_empty());
        assert!(app.world().resource::<SecretPrompt>().active.is_none());
    }

    // ── ケース 2: retry 可能 — error が設定されていても再 submit で error が消える ──
    // backend から失敗応答が返った後、ユーザーが再入力して再 submit する。
    {
        let (mut app, mut rx) = make_app();
        activate(&mut app, "req-k8-b");
        // backend から失敗応答を模擬 (error を直接設定。active は残ったまま)
        {
            let mut p = app.world_mut().resource_mut::<SecretPrompt>();
            p.error = Some("SECOND_SECRET_INVALID".to_string());
        }
        type_into_modal(&mut app, "correct");
        app.update(); // input drain
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update(); // button system

        let cmd = rx.try_recv().expect("retry submit must fire SubmitSecret");
        assert!(matches!(cmd, TransportCommand::SubmitSecret { .. }));
        // do_submit が error を None にクリアしてから送信し prompt を閉じる
        let p = app.world().resource::<SecretPrompt>();
        assert!(p.active.is_none(), "prompt must close after retry submit");
        assert!(p.error.is_none(), "error must be cleared on submit");
    }

    // ── ケース 3: Escape で cancel — command は送られず prompt + buffer が閉じる ──
    {
        let (mut app, mut rx) = make_app();
        activate(&mut app, "req-k8-c");

        type_into_modal(&mut app, "typed");
        app.update(); // drain で buffer 充填 + secret を z=300 で stack に push
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update(); // modal_layer_esc_system が secret(z=300) を pop → reconcile が do_cancel

        assert!(
            rx.try_recv().is_err(),
            "Escape (cancel) must not fire any command"
        );
        assert!(app.world().resource::<SecretPrompt>().active.is_none());
        assert!(app.world().resource::<SecretInput>().is_empty());
    }

    // ── ケース 4: Enter キーで submit — button を使わず Enter キーで送信できる ──
    {
        let (mut app, mut rx) = make_app();
        activate(&mut app, "req-k8-d");

        type_into_modal(&mut app, "pw");
        app.update(); // drain chars
        send_enter(&mut app);
        app.update(); // drain Enter → do_submit

        let cmd = rx.try_recv().expect("Enter must fire SubmitSecret");
        match cmd {
            TransportCommand::SubmitSecret { secret, .. } => {
                assert_eq!(secret.expose(), "pw");
            }
            other => panic!("expected SubmitSecret, got {other:?}"),
        }
    }

    // ── ケース 5: Cancel ボタン — error があっても command なし、prompt が閉じる ──
    {
        let (mut app, mut rx) = make_app();
        activate(&mut app, "req-k8-e");
        app.world_mut().resource_mut::<SecretPrompt>().error =
            Some("SECOND_SECRET_INVALID".to_string());

        type_into_modal(&mut app, "abc");
        app.update(); // drain
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Cancel));
        app.update(); // button system

        assert!(rx.try_recv().is_err(), "Cancel must not fire a command");
        let p = app.world().resource::<SecretPrompt>();
        assert!(p.active.is_none());
        assert!(p.error.is_none(), "close() must also clear the error");
    }
}
