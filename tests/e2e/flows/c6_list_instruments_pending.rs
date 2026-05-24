//! C6 list_instruments_pending — cold-store warming は赤エラーでなく Loading spinner。
//!
//! Issue #32 Slice 2: Manual + venue CONNECTED で銘柄 picker を開いたとき、backend の
//! InstrumentsScheduler が初回 master download 中（cold store）だと backend は
//! 60s ブロッキング fetch をせず `error_message="LIVE_UNIVERSE_PENDING"` を返す。
//! Rust 側はこのセンチネルだけを `TickersStatus::PendingLiveUniverse`（既存の "Loading..." spinner）に
//! マップし、赤い `Error:` を出さない。それ以外の失敗は従来どおり `Failed`（C2）のままにする。
//! どちらの経路でも以前ロード済みの list は破棄しない（stale 表示）。
//! 詳細は `tests/e2e/FLOWS.md` の C6 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, Ticker, TickersSource, TickersStatus};

#[test]
fn c6_list_instruments_pending() {
    let mut h = Harness::new();

    // 以前ロード済みの list（stale）。pending でも失敗でも破棄されないことを後で確認する。
    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::ReplayCatalogFallback,
        instruments: vec![Ticker {
            id: "7203.TSE".into(),
            name: "Toyota".into(),
            market: "TSE".into(),
        }],
    });

    // cold-store warming: backend は PENDING センチネルを返す。
    h.send_status(BackendStatusUpdate::InstrumentsListFailed {
        source: TickersSource::LiveVenue,
        error: "LIVE_UNIVERSE_PENDING".to_string(),
    });

    let t = h.tickers();
    assert_eq!(
        t.status,
        TickersStatus::PendingLiveUniverse,
        "PENDING は warming 中なので Loading spinner（PendingLiveUniverse）にし、赤エラーにしない"
    );
    assert_eq!(t.source, TickersSource::LiveVenue);
    assert_eq!(t.list.len(), 1, "pending 中も stale list を保持する");
    assert_eq!(t.list[0].id, "7203.TSE");

    // 対比: PENDING 以外の失敗は従来どおり赤エラー（Failed）にする（C2 の不変条件を壊さない）。
    h.send_status(BackendStatusUpdate::InstrumentsListFailed {
        source: TickersSource::LiveVenue,
        error: "instruments fetch timed out after 60s".to_string(),
    });
    let t = h.tickers();
    assert_eq!(
        t.status,
        TickersStatus::Failed("instruments fetch timed out after 60s".to_string()),
        "PENDING 以外は Failed のまま（赤エラー）"
    );
    assert_eq!(t.list.len(), 1, "失敗時も stale list を保持する");
}
