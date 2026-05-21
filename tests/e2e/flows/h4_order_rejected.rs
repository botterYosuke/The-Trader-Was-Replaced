//! H4 order_rejected — 発注が拒否されると整形メッセージで feedback に出ること。
//!
//! Manual モードの注文フォームを本番経路で駆動して `PlaceOrder` を送る。backend が
//! `OrderRejected{action, error_code}` を返すと `OrderFeedback.message` が
//! 「{action}が拒否されました ({error_code})」という整形済み文字列になることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の H4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand};

#[test]
fn h4_order_rejected() {
    let mut h = Harness::new();
    let cmds = h.place_order_via_ui("1301.TSE");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::PlaceOrder { .. })),
        "[発注]→[Confirm] は PlaceOrder を送るはず (got {cmds:?})"
    );

    h.send_status(BackendStatusUpdate::OrderRejected {
        action: "発注".to_string(),
        error_code: "EXECUTION_MODE_PRECONDITION".to_string(),
    });

    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("発注が拒否されました (EXECUTION_MODE_PRECONDITION)")
    );
}
