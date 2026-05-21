//! H2 order_status_updated — 注文 status 更新が seed 済みレコードにマージされること。
//!
//! `OrderStatusUpdated` が `client_order_id` 一致レコードに status / fill を
//! マージし、seed 済みの static フィールド（symbol / qty 等）を保持・重複挿入
//! しないことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の H2 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn h2_order_status_updated() {
    let mut h = Harness::new();
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

    h.send_status(BackendStatusUpdate::OrderStatusUpdated {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        status: "FILLED".to_string(),
        filled_qty: 100.0,
        avg_price: 1499.0,
        ts_ms: 2000,
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1, "merge must not insert a duplicate");
    let o = &orders[0];
    assert_eq!(o.status, "FILLED");
    assert_eq!(o.filled_qty, 100.0);
    assert_eq!(o.avg_price, 1499.0);
    // Static fields from the seed survive the merge.
    assert_eq!(o.symbol, "1301.TSE");
    assert_eq!(o.qty, 100.0);
}
