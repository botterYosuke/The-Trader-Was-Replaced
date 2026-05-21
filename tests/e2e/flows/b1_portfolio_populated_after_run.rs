//! B1 portfolio_populated_after_run — 実行後にポートフォリオが反映されること。
//!
//! `PortfolioLoaded` で `PortfolioState.loaded` が true になり、positions /
//! orders / equity / buying_power が充填されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の B1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, PortfolioOrder, PortfolioPosition};

#[test]
fn b1_portfolio_populated_after_run() {
    let mut h = Harness::new();
    assert!(!h.portfolio().loaded);

    h.send_status(BackendStatusUpdate::PortfolioLoaded {
        buying_power: 100_000.0,
        cash: 50_000.0,
        equity: 150_000.0,
        positions: vec![PortfolioPosition {
            symbol: "1301.TSE".to_string(),
            qty: 100,
            avg_price: 1500.0,
            unrealized_pnl: 250.0,
        }],
        orders: vec![PortfolioOrder {
            symbol: "1301.TSE".to_string(),
            side: "BUY".to_string(),
            qty: 100.0,
            price: 1500.0,
            status: "FILLED".to_string(),
            ts_ms: 1000,
        }],
    });

    let p = h.portfolio();
    assert!(p.loaded);
    assert_eq!(p.equity, 150_000.0);
    assert_eq!(p.buying_power, 100_000.0);
    assert_eq!(p.positions.len(), 1);
    assert_eq!(p.positions[0].symbol, "1301.TSE");
    assert_eq!(p.orders.len(), 1);
    assert_eq!(p.orders[0].status, "FILLED");
}
