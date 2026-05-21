//! F2 unsubscribe_market_data — 銘柄を外すと購読解除要求が出て価格がクリアされること。
//!
//! Live モードでサイドバーの × ボタンを本番 `instrument_remove_button_system` で押すと
//! `InstrumentRegistry` から銘柄が消え、`unsubscribe_removed_instruments_system` が差分を
//! 検知して `TransportCommand::UnsubscribeMarketData` を送る。transport channel で観測した後、
//! backend が当該銘柄を含まない `LastPricesUpdated` を押すと `LastPrices.map` から価格が消える
//! ことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, ExecutionMode, TransportCommand};
use backcast::ui::components::SidebarInstrumentRemoveButton;
use std::collections::HashMap;

#[test]
fn f2_unsubscribe_market_data() {
    let mut h = Harness::new();
    // Live モードで universe に 7203 が居る状態を作る（prime tick で diff 検知器を整える）。
    h.set_exec_mode(ExecutionMode::LiveManual);
    h.set_instruments(&["7203.TSE"]);
    h.tick();

    let mut prices = HashMap::new();
    prices.insert("7203.TSE".to_string(), 2500.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });
    assert!(h.last_prices().map.contains_key("7203.TSE"));

    // ユーザーが × ボタンで 7203 を外す → 差分検知で UnsubscribeMarketData。
    h.click(SidebarInstrumentRemoveButton {
        instrument_id: "7203.TSE".to_string(),
    });
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::UnsubscribeMarketData { instrument_id } if instrument_id == "7203.TSE"
        )),
        "× ボタンは UnsubscribeMarketData を送るはず (got {cmds:?})"
    );

    // backend が当該銘柄を含まない更新を push → map から消える。
    h.send_status(BackendStatusUpdate::LastPricesUpdated {
        prices: HashMap::new(),
    });
    assert!(
        h.last_prices().map.is_empty(),
        "unsubscribe clears the price map"
    );
}
