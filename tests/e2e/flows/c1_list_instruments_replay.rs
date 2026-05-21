//! C1 list_instruments_replay — リプレイ用銘柄一覧の取得が成功するフロー。
//!
//! `InstrumentsListStarted` で `TickersStatus` が InFlight になり、
//! `InstrumentsListed` で Loaded になって `Tickers.list` が充填され、source
//! （ReplayCatalogFallback）が記録されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の C1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, Ticker, TickersSource, TickersStatus};

#[test]
fn c1_list_instruments_replay() {
    let mut h = Harness::new();
    assert_eq!(h.tickers().status, TickersStatus::NotFetched);

    h.send_status(BackendStatusUpdate::InstrumentsListStarted {
        source: TickersSource::ReplayCatalogFallback,
    });
    assert_eq!(h.tickers().status, TickersStatus::InFlight);

    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![
            Ticker { id: "1301.TSE".into(), name: "Kyokuyo".into(), market: "TSE".into() },
            Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() },
        ],
    });
    let t = h.tickers();
    assert_eq!(t.status, TickersStatus::Loaded);
    assert_eq!(t.source, TickersSource::ReplayCatalogFallback);
    assert_eq!(t.list.len(), 2);
}
