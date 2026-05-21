//! K6 reconcile_modal_after_backend_restart — backend 再起動後、追跡されていない working 注文だけが reconcile モーダルに入ること。
//!
//! `OrdersReconciled{backend_client_order_ids}` を楽観的 UI の `LiveOrders` と
//! diff し、backend が追跡していない working 注文だけが `ReconcilePrompt.unknown`
//! に入る。terminal（FILLED 等）の注文は無視されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の K6 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn k6_reconcile_modal_after_backend_restart() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "working-missing".to_string(),
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
    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "working-present".to_string(),
        venue_order_id: "v-2".to_string(),
        symbol: "7203.TSE".to_string(),
        side: "SELL".to_string(),
        qty: 10.0,
        price: Some(2500.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 1000,
    });
    h.send_status(BackendStatusUpdate::OrderSeeded {
        client_order_id: "filled-missing".to_string(),
        venue_order_id: "v-3".to_string(),
        symbol: "9984.TSE".to_string(),
        side: "BUY".to_string(),
        qty: 1.0,
        price: Some(7000.0),
        status: "FILLED".to_string(),
        filled_qty: 1.0,
        avg_price: 7000.0,
        ts_ms: 1000,
    });

    h.send_status(BackendStatusUpdate::OrdersReconciled {
        backend_client_order_ids: vec!["working-present".to_string()],
    });

    let unknown = h.reconcile_prompt().unknown;
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].client_order_id, "working-missing");
    assert_eq!(unknown[0].symbol, "1301.TSE");
    assert_eq!(unknown[0].status, "WORKING");
}
