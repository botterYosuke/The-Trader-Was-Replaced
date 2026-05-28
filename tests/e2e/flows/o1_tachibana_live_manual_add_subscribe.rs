//! O1 tachibana_live_manual_add_subscribe — TACHIBANA ログイン → Manual モード →
//! live universe ロード → サイドバー行クリック → チャート&板の自動更新確認（kind:state + kind:ui）。
//!
//! ## シナリオ（ユーザー操作のトレース）
//!
//! 1. TACHIBANA demo に接続（Venue→Connect）→ backend が VenueChanged(Connected) を返す
//! 2. Manual モードへ切り替え（ExecutionMode::LiveManual）
//! 3. backend が live universe を InstrumentsListed で返す（[+ Add] ピッカーが表示する候補元）
//! 4. ユーザーがサイドバー行をクリック → SubscribeMarketData が送られること
//! 5. backend がチャート&板データ（LastPricesUpdated）を push → LastPrices に反映されること
//!
//! ## 観測ポイント
//!
//! - `VenueStatusRes.state == Connected && venue_id == "tachibana"` — ログイン確認
//! - `ExecutionModeRes.mode == LiveManual` — Manual モード確認
//! - `Tickers.list` に venue 銘柄が入る — universe ロード確認
//! - `TransportCommand::SubscribeMarketData` が送出される — 購読要求確認
//! - `SelectedSymbol` が更新される — 選択状態確認
//! - `LastPrices.map` に価格が充填される — チャート&板の自動更新確認
//!
//! [+ Add] ピッカー固有の UI（候補行 spawn / Escape 閉じ / query 絞り込み）は J11 がカバー。
//! Live universe auto-fetch コマンド発射は `auto_fetch_live_universe_on_connect_system` 単体で担保。
//! 本テストはそれらを統合した「TACHIBANA 接続からデータ表示まで」のエンドツーエンド回帰ガード。

use crate::support::Harness;
use backcast::trading::{
    BackendStatusUpdate, ExecutionMode, Ticker, TickersSource, TransportCommand, VenueState,
};
use backcast::ui::components::SidebarInstrumentRowClick;
use std::collections::HashMap;

#[test]
fn o1_tachibana_live_manual_add_subscribe() {
    let mut h = Harness::new();

    // ── Step 1: backend 接続 + TACHIBANA ログイン成功 ─────────────────────────
    h.send_status(BackendStatusUpdate::Connected(true));
    h.set_venue(VenueState::Connected, "tachibana");
    {
        let v = h.venue();
        assert_eq!(v.state, VenueState::Connected, "TACHIBANA ログイン後: Connected であること");
        assert_eq!(
            v.venue_id.as_deref(),
            Some("tachibana"),
            "venue_id が 'tachibana' に記録されること"
        );
    }

    // ── Step 2: Manual モードへ切り替え ──────────────────────────────────────
    h.set_exec_mode(ExecutionMode::LiveManual);
    assert_eq!(
        h.exec_mode().mode,
        ExecutionMode::LiveManual,
        "ExecutionMode が LiveManual になること"
    );

    // ── Step 3: TACHIBANA live universe ロード（[+ Add] ピッカーの候補元）──────
    // auto_fetch_live_universe_on_connect_system が ListInstruments を送り、
    // backend が InstrumentsListed で応答する経路を模倣。
    h.send_status(BackendStatusUpdate::InstrumentsListed {
        source: TickersSource::LiveVenue,
        instruments: vec![
            Ticker {
                id: "7203.TSE".into(),
                name: "トヨタ自動車".into(),
                market: "TSE".into(),
            },
            Ticker {
                id: "6758.TSE".into(),
                name: "ソニーグループ".into(),
                market: "TSE".into(),
            },
        ],
    });
    {
        let tickers = h.tickers();
        assert!(
            tickers.list.iter().any(|t| t.id == "7203.TSE"),
            "InstrumentsListed で 7203.TSE が Tickers.list に登録されること (got {:?})",
            tickers.list
        );
        assert!(
            tickers.list.iter().any(|t| t.id == "6758.TSE"),
            "InstrumentsListed で 6758.TSE が Tickers.list に登録されること (got {:?})",
            tickers.list
        );
    }

    // ── Step 4: サイドバー行クリック → SubscribeMarketData 送出 ─────────────
    // [+ Add] ピッカーで銘柄を選択した後、サイドバー行をクリックして購読要求を出す。
    // Live モードでは instrument_row_click_system が SubscribeMarketData を送る（Replay は送らない）。
    h.click(SidebarInstrumentRowClick {
        instrument_id: "7203.TSE".to_string(),
    });
    {
        let cmds = h.drain_commands();
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                TransportCommand::SubscribeMarketData { instrument_id }
                    if instrument_id == "7203.TSE"
            )),
            "LiveManual での行クリックは SubscribeMarketData を送るはず (got {cmds:?})"
        );
        assert_eq!(
            h.selected_symbol().as_deref(),
            Some("7203.TSE"),
            "行クリックで SelectedSymbol が 7203.TSE に更新されること"
        );
    }

    // ── Step 5: backend がチャート&板データを push → 自動更新確認 ────────────
    // 購読後、backend は venue tick ストリームから LastPricesUpdated を push する。
    // これがサイドバー最終値・チャート直近値・板（ラダー）表示の「自動更新」の seam。
    let mut prices = HashMap::new();
    prices.insert("7203.TSE".to_string(), 3_450.0);
    prices.insert("6758.TSE".to_string(), 2_980.0);
    h.send_status(BackendStatusUpdate::LastPricesUpdated { prices });
    {
        let lp = h.last_prices();
        assert_eq!(
            lp.map.get("7203.TSE"),
            Some(&3_450.0),
            "チャート&板: 7203.TSE の最新値が LastPrices に反映されること"
        );
        assert_eq!(
            lp.map.get("6758.TSE"),
            Some(&2_980.0),
            "チャート&板: 6758.TSE の最新値も LastPrices に反映されること"
        );
    }
}
