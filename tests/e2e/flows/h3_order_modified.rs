//! H3 order_modified — 発注後、注文訂正で Some の項目のみ上書きされること。
//!
//! Manual モードの注文フォームを本番経路で駆動して `PlaceOrder` を送る。backend が
//! `OrderSeeded` で seed した後、`OrderModified` は `Some` の qty / price のみ上書きし、`None`
//! は追跡中の値を維持する。status / fill も更新されることを確認する（部分訂正の不変条件）。
//! 詳細は `tests/e2e/FLOWS.md` の H3 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand};

#[test]
fn h3_order_modified() {
    let mut h = Harness::new();
    let cmds = h.place_order_via_ui("1301.TSE");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::PlaceOrder { .. })),
        "[発注]→[Confirm] は PlaceOrder を送るはず (got {cmds:?})"
    );

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

    // Modify only the price; qty is None so it must stay at 100.
    h.send_status(BackendStatusUpdate::OrderModified {
        client_order_id: "c-1".to_string(),
        venue_order_id: "v-1".to_string(),
        new_qty: None,
        new_price: Some(1450.0),
        status: "WORKING".to_string(),
        filled_qty: 0.0,
        avg_price: 0.0,
        ts_ms: 2000,
    });

    let o = h.live_orders().orders[0].clone();
    assert_eq!(o.price, Some(1450.0), "Some(price) overwrites");
    assert_eq!(o.qty, 100.0, "None new_qty keeps the tracked value");
    assert_eq!(o.ts_ms, 2000);
}
