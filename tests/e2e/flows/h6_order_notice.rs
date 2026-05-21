//! H6 order_notice — 発注後、構造化拒否でない注文 notice が verbatim 表示されること。
//!
//! Manual モードの注文フォームを本番経路で駆動して `PlaceOrder` を送る。backend が
//! `OrderNotice{message}` を返すと `OrderFeedback.message` にそのまま（整形せず）出ることを
//! 確認する。incomplete success / transport error など、構造化 reject ではない注文フローの通知。
//! 詳細は `tests/e2e/FLOWS.md` の H6 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand};

#[test]
fn h6_order_notice() {
    let mut h = Harness::new();
    let cmds = h.place_order_via_ui("1301.TSE");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::PlaceOrder { .. })),
        "[発注]→[Confirm] は PlaceOrder を送るはず (got {cmds:?})"
    );

    h.send_status(BackendStatusUpdate::OrderNotice {
        message: "注文は受け付けられましたが状態を追跡できません".to_string(),
    });

    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("注文は受け付けられましたが状態を追跡できません")
    );
}
