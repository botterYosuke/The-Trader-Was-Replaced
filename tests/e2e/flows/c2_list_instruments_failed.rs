//! C2 list_instruments_failed — 銘柄一覧の取得失敗時に旧リストを保持すること（stale 表示）。
//!
//! `InstrumentsListFailed` で `TickersStatus::Failed` になっても、以前ロード済み
//! の list は破棄されず保持される（取得失敗で画面が空にならない）ことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の C2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, Ticker, TickersSource, TickersStatus};

#[test]
fn c2_list_instruments_failed() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![Ticker {
            id: "7203.TSE".into(),
            name: "Toyota".into(),
            market: "TSE".into(),
        }],
    });

    h.send_status(BackendStatusUpdate::InstrumentsListFailed {
        source: TickersSource::LiveVenue,
        error: "grpc timeout".to_string(),
    });

    let t = h.tickers();
    assert_eq!(t.status, TickersStatus::Failed("grpc timeout".to_string()));
    assert_eq!(t.source, TickersSource::LiveVenue);
    assert_eq!(t.list.len(), 1, "stale list must be retained");
    assert_eq!(t.list[0].id, "7203.TSE");
}
