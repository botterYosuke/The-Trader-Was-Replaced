//! F1 subscribe_market_data — Live モードで銘柄行をクリックすると購読要求が出て最新値が入ること。
//!
//! 実サイドバーの銘柄行を本番 `instrument_row_click_system` で駆動する。Live モードでは
//! クリックで `SelectedSymbol` が更新され `TransportCommand::SubscribeMarketData` が送られる。
//! transport channel でコマンドを観測した後、backend が `LastPricesUpdated{prices}` を押すと
//! `LastPrices.map` が充填され銘柄ごとの最新値が引けることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, ExecutionMode, TransportCommand};
use backcast::ui::components::SidebarInstrumentRowClick;
use std::collections::HashMap;

#[test]
fn f1_subscribe_market_data() {
    let mut h = Harness::new();
    assert!(h.last_prices().map.is_empty());

    // Live モードで 7203.TSE の行をクリック。
    h.set_exec_mode(ExecutionMode::LiveManual);
    h.click(SidebarInstrumentRowClick {
        instrument_id: "7203.TSE".to_string(),
    });
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::SubscribeMarketData { instrument_id } if instrument_id == "7203.TSE"
        )),
        "Live モードの行クリックは SubscribeMarketData を送るはず (got {cmds:?})"
    );
    assert_eq!(h.selected_symbol().as_deref(), Some("7203.TSE"));

    // backend が購読銘柄の最新値を push する。
    let mut prices = HashMap::new();
    prices.insert("7203.TSE".to_string(), 2500.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });

    assert_eq!(h.last_prices().map.get("7203.TSE"), Some(&2500.0));
}
