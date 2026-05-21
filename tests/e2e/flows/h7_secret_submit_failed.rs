//! H7 secret_submit_failed — シークレット送信失敗が SecretModal 側に出て注文 feedback を汚さないこと。
//!
//! 発注 → backend の `SecretRequired` でモーダルを開き、ユーザーが本番 SecretModal に第二暗証を
//! 入力して送信する（`secret_modal_input_system` → `secret_modal_button_system` が `SubmitSecret`
//! を送る）。その後 backend が `SecretSubmitFailed{error_code}` を返すと、retry 可能な error が
//! `SecretPrompt.error` にセットされ（SecretModal から再試行できる）、既存の `OrderFeedback.message`
//! は汚染しないことを確認する。secret フローの失敗と OrderPanel の feedback バケツを分離する不変条件。
//! 詳細は `tests/e2e/FLOWS.md` の H7 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, BackendStatusUpdate, TransportCommand};
use backcast::ui::secret_modal::SecretButton;

#[test]
fn h7_secret_submit_failed() {
    let mut h = Harness::new();

    // 発注 → backend が第二暗証要求 → モーダルが開く。
    h.place_order_via_ui("7203.TSE");
    h.send_event(BackendEvent::SecretRequired {
        request_id: "req-1".to_string(),
        venue: "tachibana".to_string(),
        kind: "second_password".to_string(),
        purpose: "place_order".to_string(),
    });
    assert!(h.secret_prompt().active.is_some(), "SecretModal が開くはず");

    // ユーザーが第二暗証を入力して送信 → SubmitSecret コマンド。
    h.type_secret("1234");
    h.click(SecretButton::Submit);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::SubmitSecret { request_id, .. } if request_id == "req-1"
        )),
        "SecretModal の送信は SubmitSecret を送るはず (got {cmds:?})"
    );

    // OrderPanel に既存の notice がある状態を作る。
    h.send_status(BackendStatusUpdate::OrderNotice {
        message: "prior order notice".to_string(),
    });

    // backend が secret 送信失敗を返す。
    h.send_status(BackendStatusUpdate::SecretSubmitFailed {
        error_code: "SECOND_SECRET_INVALID".to_string(),
    });

    assert!(
        h.secret_prompt()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("SECOND_SECRET_INVALID")),
        "secret submit failure must be retryable from the SecretModal"
    );
    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("prior order notice"),
        "secret-flow failures must not overwrite the OrderPanel feedback bucket"
    );
}
