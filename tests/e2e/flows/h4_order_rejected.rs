//! H4 order_rejected — 注文拒否が整形メッセージで feedback に出ること。
//!
//! `OrderRejected{action, error_code}` で `OrderFeedback.message` が
//! 「{action}が拒否されました ({error_code})」という整形済み文字列になること
//! を確認する。
//! 詳細は `tests/e2e/FLOWS.md` の H4 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn h4_order_rejected() {
    let mut h = Harness::new();
    assert!(h.order_feedback().message.is_none());

    h.send_status(BackendStatusUpdate::OrderRejected {
        action: "発注".to_string(),
        error_code: "EXECUTION_MODE_PRECONDITION".to_string(),
    });

    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("発注が拒否されました (EXECUTION_MODE_PRECONDITION)")
    );
}
