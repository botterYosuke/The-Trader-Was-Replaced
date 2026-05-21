//! F1 subscribe_market_data — 市場データ購読で最新値が入ること。
//!
//! `LastPricesUpdated{prices}` で `LastPrices.map` が充填され、銘柄ごとの最新値
//! が引けることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F1 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn f1_subscribe_market_data() {
    let mut h = Harness::new();
    assert!(h.last_prices().map.is_empty());

    let mut prices = std::collections::HashMap::new();
    prices.insert("7203.TSE".to_string(), 2500.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });

    assert_eq!(h.last_prices().map.get("7203.TSE"), Some(&2500.0));
}
