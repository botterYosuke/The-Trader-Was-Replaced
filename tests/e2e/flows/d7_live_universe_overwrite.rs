//! D7 live_universe_overwrite — Live 銘柄ユニバースが Replay fallback を丸ごと上書きすること。
//!
//! ReplayCatalogFallback の list がある状態で LiveVenue の `InstrumentsListed`
//! が来ると、union ではなく wholesale 上書きされる（plan §0.5.1 の不変条件）。
//! fallback の銘柄が残らないことも確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D7 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, Ticker, TickersSource};

#[test]
fn d7_live_universe_overwrite() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![
            Ticker { id: "1301.TSE".into(), name: "Kyokuyo".into(), market: "TSE".into() },
            Ticker { id: "7203.TSE".into(), name: "Toyota".into(), market: "TSE".into() },
        ],
    });

    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::LiveVenue,
        instruments: vec![Ticker {
            id: "9984.TSE".into(),
            name: "SoftBank".into(),
            market: "TSE".into(),
        }],
    });

    let t = h.tickers();
    assert_eq!(t.source, TickersSource::LiveVenue);
    assert_eq!(t.list.len(), 1, "live universe must overwrite the fallback list");
    assert_eq!(t.list[0].id, "9984.TSE");
    assert!(
        !t.list.iter().any(|x| x.id == "1301.TSE"),
        "fallback entries must not survive the overwrite"
    );
}
