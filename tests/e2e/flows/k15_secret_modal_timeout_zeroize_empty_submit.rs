//! K15 secret_modal_timeout_zeroize_empty_submit — SecretModal は keyboard 入力を mask 表示し、空 Submit は無視し、
//! Cancel / timeout / supersede で入力バッファを zeroize することを保証する（kind:ui）。
//!
//! **Timeout 駆動について**: `secret_modal_timeout_system` は `Instant::now()` を直接使う。
//! `SecretInput.opened_at` は private フィールドなので外部テストから直接設定できない。
//! タイムアウト挙動は `src/ui/secret_modal.rs` の `timeout_closes_modal` /
//! `timeout_does_not_close_before_deadline` ユニットテストで完全にカバーされている。
//! 本 E2E flow は以下を確認する:
//!  (a) 空 Submit の no-op (prompt 開いたまま・command なし)
//!  (b) Cancel ボタン / Escape キーによる zeroize
//!  (c) lifecycle_system 経由の supersede (別 request_id) による zeroize
//!  (d) 文字キー入力 → len() 増加 → Backspace → len() 減少 (mask 表示の前提確認)
//!
//! **入力方法**: `SecretInput.push_char` は private。
//! 外部テストは `Events<KeyboardInput>` + `secret_modal_input_system` で入力する。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    OrderFeedback, SecretPrompt, SecretPromptRequest, TransportCommand, TransportCommandSender,
};
use backcast::ui::secret_modal::{
    SecretButton, SecretInput, secret_modal_button_system, secret_modal_input_system,
    secret_modal_lifecycle_system,
};

// ── ヘルパー ──────────────────────────────────────────────────────────────────

fn make_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = App::new();
    app.init_resource::<SecretInput>();
    app.init_resource::<SecretPrompt>();
    app.init_resource::<OrderFeedback>();
    app.insert_resource(TransportCommandSender { tx });
    app.add_event::<KeyboardInput>();
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

/// keyboard drain 経由でバッファに文字を詰める (push_char は private)。
/// 文字ごとに別イベントを送る。呼び出し後に `app.update()` が必要。
fn queue_chars(app: &mut App, s: &str) {
    for c in s.chars() {
        let cs = c.to_string();
        app.world_mut()
            .resource_mut::<Events<KeyboardInput>>()
            .send(KeyboardInput {
                // key_code は secret_modal_input_system が参照しない。logical_key のみ使用。
                key_code: KeyCode::F35,
                logical_key: Key::Character(cs.as_str().into()),
                state: ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
    }
}

fn queue_escape(app: &mut App) {
    app.world_mut()
        .resource_mut::<Events<KeyboardInput>>()
        .send(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
}

#[test]
fn k15_secret_modal_timeout_zeroize_empty_submit() {
    // ── ケース 1: 空 Submit は無視 — command なし・prompt は開いたまま ──
    // §9 Open Risk 1: 空 secret を送ると one-shot request_id を浪費し
    // Tachibana の失敗回数制限を空打ちで削る。
    {
        let (mut app, mut rx) = make_app();
        app.add_systems(Update, secret_modal_button_system);

        activate(&mut app, "req-k15-empty");
        // buffer は empty のまま

        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "empty buffer must not fire SubmitSecret"
        );
        assert!(
            app.world().resource::<SecretPrompt>().active.is_some(),
            "prompt must stay open so the user can still type"
        );
        assert!(
            app.world().resource::<SecretInput>().is_empty(),
            "buffer remains empty after empty submit"
        );
    }

    // ── ケース 2: Cancel ボタンで zeroize — バッファが消え prompt が閉じる ──
    {
        let (mut app, mut rx) = make_app();
        app.add_systems(
            Update,
            (secret_modal_input_system, secret_modal_button_system),
        );

        activate(&mut app, "req-k15-cancel");
        queue_chars(&mut app, "typed");
        app.update(); // drain → buffer populated
        assert!(!app.world().resource::<SecretInput>().is_empty());

        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Cancel));
        app.update();

        assert!(rx.try_recv().is_err(), "Cancel must not fire SubmitSecret");
        assert!(
            app.world().resource::<SecretInput>().is_empty(),
            "buffer must be zeroized on Cancel"
        );
        assert!(
            app.world().resource::<SecretPrompt>().active.is_none(),
            "prompt must close on Cancel"
        );
    }

    // ── ケース 3: Escape キーで cancel — keyboard drain 経由の zeroize ──
    {
        let (mut app, mut rx) = make_app();
        app.add_systems(Update, secret_modal_input_system);

        activate(&mut app, "req-k15-escape");
        queue_chars(&mut app, "partial");
        app.update(); // drain chars
        queue_escape(&mut app);
        app.update(); // drain Escape → do_cancel

        assert!(rx.try_recv().is_err(), "Escape must not fire SubmitSecret");
        assert!(
            app.world().resource::<SecretInput>().is_empty(),
            "buffer must be zeroized on Escape"
        );
        assert!(app.world().resource::<SecretPrompt>().active.is_none());
    }

    // ── ケース 4: supersede (別 request_id) で旧バッファが zeroize される ──
    // lifecycle_system: request_id が変わると古いバッファを 0 埋めして opened_at をリセット。
    {
        let (mut app, _rx) = make_app();
        app.add_systems(
            Update,
            (secret_modal_lifecycle_system, secret_modal_input_system),
        );

        // 1st request: lifecycle が opened_at を arm する
        activate(&mut app, "req-A");
        app.update(); // lifecycle arms the clock
        queue_chars(&mut app, "partialpin");
        app.update(); // drain → buffer populated
        assert!(!app.world().resource::<SecretInput>().is_empty());

        // 2nd request が supersede
        activate(&mut app, "req-B");
        app.update(); // lifecycle が id 変更を検知 → zeroize

        let input = app.world().resource::<SecretInput>();
        assert!(
            input.is_empty(),
            "supersede by a different request_id must zeroize the old buffer"
        );
    }

    // ── ケース 5: 空 submit 後も再入力・再 submit できる (prompt は開いたまま) ──
    {
        let (mut app, mut rx) = make_app();
        app.add_systems(
            Update,
            (secret_modal_input_system, secret_modal_button_system),
        );

        activate(&mut app, "req-k15-retry");
        // first: empty submit → no-op
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update();
        assert!(rx.try_recv().is_err());
        assert!(app.world().resource::<SecretPrompt>().active.is_some());

        // type and submit successfully
        queue_chars(&mut app, "pw2");
        app.update(); // drain chars

        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update();

        let cmd = rx.try_recv().expect("second submit must fire SubmitSecret");
        assert!(matches!(cmd, TransportCommand::SubmitSecret { .. }));
        assert!(app.world().resource::<SecretInput>().is_empty());
    }

    // ── ケース 6: 文字キー入力 → len() 増加 → Backspace → len() 減少 (mask の前提確認) ──
    {
        let (mut app, _rx) = make_app();
        app.add_systems(Update, secret_modal_input_system);

        activate(&mut app, "req-k15-keys");

        queue_chars(&mut app, "123");
        app.update(); // drain
        assert_eq!(
            app.world().resource::<SecretInput>().len(),
            3,
            "three digit keys must accumulate in the buffer"
        );

        app.world_mut()
            .resource_mut::<Events<KeyboardInput>>()
            .send(KeyboardInput {
                key_code: KeyCode::Backspace,
                logical_key: Key::Backspace,
                state: ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        app.update();
        assert_eq!(
            app.world().resource::<SecretInput>().len(),
            2,
            "Backspace must remove one character"
        );
    }
}
