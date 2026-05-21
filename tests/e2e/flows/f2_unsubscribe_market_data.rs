//! F2 unsubscribe_market_data — 購読解除で価格がクリアされること。
//!
//! 後続の `LastPricesUpdated` が当該銘柄を含まない（購読解除＝空 or 縮小した）
//! map のとき、`LastPrices.map` から価格が消えることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F2 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn f2_unsubscribe_market_data() {
    let mut h = Harness::new();
    let mut prices = std::collections::HashMap::new();
    prices.insert("7203.TSE".to_string(), 2500.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });
    assert!(h.last_prices().map.contains_key("7203.TSE"));

    h.send_status(BackendStatusUpdate::LastPricesUpdated {
        prices: std::collections::HashMap::new(),
    });
    assert!(h.last_prices().map.is_empty(), "unsubscribe clears the price map");
}
