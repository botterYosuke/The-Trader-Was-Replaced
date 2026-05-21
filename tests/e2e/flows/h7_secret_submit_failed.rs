//! H7 secret_submit_failed — シークレット送信失敗が SecretModal 側に出て注文 feedback を汚さないこと。
//!
//! `SecretSubmitFailed{error_code}` は retry 可能な error を `SecretPrompt.error`
//! にセットし（SecretModal から再試行できる）、`OrderFeedback.message` は汚染
//! しないことを確認する。secret フローの失敗と OrderPanel の feedback バケツを
//! 分離する不変条件。
//! 詳細は `tests/e2e/FLOWS.md` の H7 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn h7_secret_submit_failed() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::OrderNotice {
        message: "prior order notice".to_string(),
    });

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
