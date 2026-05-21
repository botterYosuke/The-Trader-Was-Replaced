//! H1 order_seeded — 発注 seed で LiveOrders に full レコードが入り feedback がクリアされること。
//!
//! `OrderSeeded` は（OrderEvent には無い symbol / side / qty / price を含む）full
//! レコードを `LiveOrders` に seed し、同時に以前の `OrderFeedback` notice
//! （前回の拒否メッセージ等）をクリアすることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の H1 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn h1_order_seeded() {
    let mut h = Harness::new();
    // A stale reject notice from a prior attempt.
    h.send_status(BackendStatusUpdate::OrderRejected {
        action: "発注".to_string(),
        error_code: "VENUE_LOGIN_REQUIRED".to_string(),
    });
    assert!(h.order_feedback().message.is_some());

    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        symbol: "1301.TSE".to_string(),
        side: "BUY".to_string(),
        qty: 100.0,
        price: Some(1500.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(o.client_order_id, "c-1");
    assert_eq!(o.symbol, "1301.TSE");
    assert_eq!(o.side, "BUY");
    assert_eq!(o.qty, 100.0);
    assert_eq!(o.price, Some(1500.0));
    assert!(
        h.order_feedback().message.is_none(),
        "a successful seed clears the prior reject notice"
    );
}
