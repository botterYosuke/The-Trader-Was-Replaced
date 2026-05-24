//! H9 orders_seeded_full_rows — GetOrders スナップショットが LiveOrders に完全な注文行を seed する。
//!
//! issue #29 Slice3a: proto `OrderEvent` が symbol/side/qty/price を運ぶようになり、transport task は
//! `GetOrders` 応答から完全な `LiveOrder` 行を組んで `OrdersSeeded` で送る。`apply_status_update` は
//! `LiveOrders::seed_working` で適用する:
//!   1. UI が未知の注文（前セッション/再起動後）は完全行として挿入される。
//!   2. id だけ既知（EC stream が静的属性なしで先に挿入）の行は seed で gap-fill される。
//! feedback はこの背景 sync では触らない（注文フロー由来のイベントではない）。
//! 詳細は `tests/e2e/FLOWS.md` の H9 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, LiveOrder};

#[test]
fn h9_orders_seeded_full_rows() {
    let mut h = Harness::new();

    // 1) 未知の稼働中注文（GetOrders snapshot 由来）→ 完全行として挿入される。
    h.send_status(BackendStatusUpdate::OrdersSeeded {
        orders: vec![LiveOrder {
            client_order_id: "c-prior".to_string(),
            venue_order_id: "V-prior".to_string(),
            symbol: "7203.T".to_string(),
            side: "BUY".to_string(),
            qty: 100.0,
            price: Some(2500.0),
            status: "ACCEPTED".to_string(),
            filled_qty: 0.0,
            avg_price: 0.0,
            ts_ms: 10,
            strategy_id: "MANUAL-001".to_string(),
        }],
    });

    let prior = h
        .live_orders()
        .orders
        .into_iter()
        .find(|o| o.client_order_id == "c-prior")
        .expect("未知注文が seed されているはず");
    assert_eq!(prior.symbol, "7203.T");
    assert_eq!(prior.side, "BUY");
    assert_eq!(prior.qty, 100.0);
    assert_eq!(prior.price, Some(2500.0));
    assert_eq!(prior.status, "ACCEPTED");

    // 2) id だけ既知（EC stream が静的属性なしで先に挿入）→ seed で gap-fill される。
    h.send_status(BackendStatusUpdate::OrderStatusUpdated {
        client_order_id: "c-ec".to_string(),
        venue_order_id: "V-ec".to_string(),
        status: "ACCEPTED".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 20,
    });
    assert!(
        h.live_orders()
            .orders
            .iter()
            .find(|o| o.client_order_id == "c-ec")
            .expect("EC 由来の id-only 行")
            .symbol
            .is_empty(),
        "EC stream で挿入された行はまだ静的属性が空"
    );

    h.send_status(BackendStatusUpdate::OrdersSeeded {
        orders: vec![LiveOrder {
            client_order_id: "c-ec".to_string(),
            venue_order_id: "V-ec".to_string(),
            symbol: "6758.T".to_string(),
            side: "SELL".to_string(),
            qty: 300.0,
            price: None,
            status: "ACCEPTED".to_string(),
            filled_qty: 0.0,
            avg_price: 0.0,
            ts_ms: 30,
            strategy_id: String::new(),
        }],
    });

    let ec = h
        .live_orders()
        .orders
        .into_iter()
        .find(|o| o.client_order_id == "c-ec")
        .expect("seed 後も存在");
    assert_eq!(ec.symbol, "6758.T", "空だった symbol が seed で埋まる");
    assert_eq!(ec.side, "SELL");
    assert_eq!(ec.qty, 300.0);
    assert_eq!(ec.price, None, "MARKET 行は指値なし");

    // 背景 sync は feedback を触らない。
    assert!(h.order_feedback().message.is_none());
}
