//! F3 order_event — 注文イベントが LiveOrders に反映・マージされること。
//!
//! 未知の `client_order_id` の `OrderEvent` は static フィールド（symbol 等）を
//! 空にしてレコード挿入され、同 id の後続イベントは in-place マージされる
//! （重複挿入しない）ことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F3 を参照。

use crate::support::Harness;
use backcast::trading::BackendEvent;

#[test]
fn f3_order_event() {
    let mut h = Harness::new();
    assert!(h.live_orders().orders.is_empty());

    h.send_event(BackendEvent::OrderEvent {
        order_id: "o-1".to_string(),
        venue_order_id: "v-1".to_string(),
        client_order_id: "c-1".to_string(),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
        strategy_id: String::new(),
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(o.client_order_id, "c-1");
    assert_eq!(o.venue_order_id, "v-1");
    assert_eq!(o.status, "WORKING");
    // Unknown id inserted with empty static fields (no PlaceOrder seed yet).
    assert!(o.symbol.is_empty());
    assert_eq!(o.filled_qty, 0.0);

    // A fill update for the same client_order_id merges in place.
    h.send_event(BackendEvent::OrderEvent {
        order_id: "o-1".to_string(),
        venue_order_id: "v-1".to_string(),
        client_order_id: "c-1".to_string(),
        status: "FILLED".to_string(),
        filled_qty: 100.0,
        avg_price: 1500.0,
        ts_ms: 2000,
        strategy_id: String::new(),
    });

    let orders = h.live_orders().orders;
    assert_eq!(orders.len(), 1, "merge must not insert a duplicate");
    let o = &orders[0];
    assert_eq!(o.status, "FILLED");
    assert_eq!(o.filled_qty, 100.0);
    assert_eq!(o.avg_price, 1500.0);
}
