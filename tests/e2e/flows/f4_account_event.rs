//! F4 account_event — 接続中の口座イベントでポートフォリオが更新されること。
//!
//! Venue→Connect で接続を要求し backend が Connected を押した後、backend の `AccountEvent` で
//! `PortfolioState`（cash / buying_power / positions / loaded）が更新され、equity が
//! cash + Σ(qty*avg_price + unrealized_pnl) で導出されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F4 を参照。

use crate::support::Harness;
use backcast::trading::{AccountPosition, BackendEvent, BackendStatusUpdate, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn f4_account_event() {
    let mut h = Harness::new();
    assert!(!h.portfolio().loaded);

    // 接続済みにする（Connect → backend Connected）。
    h.click(MenuItem::VenueConnectTachibanaDemo);
    h.drain_commands();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });

    h.send_event(BackendEvent::AccountEvent {
        cash: 50_000.0,
        buying_power: 120_000.0,
        positions: vec![
            AccountPosition {
                symbol: "1301.TSE".to_string(),
                qty: 100,
                avg_price: 1500.0,
                unrealized_pnl: 250.0,
            },
            AccountPosition {
                symbol: "7203.TSE".to_string(),
                qty: 10,
                avg_price: 2000.0,
                unrealized_pnl: -100.0,
            },
        ],
        ts_ms: 1000,
    });

    let p = h.portfolio();
    assert!(p.loaded);
    assert_eq!(p.cash, 50_000.0);
    assert_eq!(p.buying_power, 120_000.0);
    assert_eq!(p.positions.len(), 2);
    assert_eq!(p.positions[0].symbol, "1301.TSE");
    // cash + (100*1500 + 250) + (10*2000 + -100) = 50000 + 150250 + 19900
    assert_eq!(p.equity, 50_000.0 + 150_250.0 + 19_900.0);
}
