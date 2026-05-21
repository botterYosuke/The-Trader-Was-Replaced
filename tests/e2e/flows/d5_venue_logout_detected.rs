//! D5 venue_logout_detected — 接続中の外部ログアウト検知で再ログインモーダルが開くこと。
//!
//! Connect → backend Connected で接続済みにした後、health watchdog が
//! `BackendEvent::VenueLogoutDetected{venue}` を押すと `ReloginPrompt.active` が
//! `Some(venue)` になり、venue が落ちたことをユーザーに通知する。モーダルは通知に徹し
//! 自身は再ログインしない（再ログインは Venue メニュー経由）ため `VenueStatusRes` は
//! 意図的に不変。
//! 詳細は `tests/e2e/FLOWS.md` の D5 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, BackendStatusUpdate, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn d5_venue_logout_detected() {
    let mut h = Harness::new();
    assert!(h.relogin_prompt().active.is_none());

    // 接続済みにする（Connect → backend Connected）。
    h.click(MenuItem::VenueConnectKabuVerify);
    h.drain_commands();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("kabu".to_string()),
        instruments_loaded: 5,
    });

    // 接続中に backend が外部ログアウトを検知。
    h.send_event(BackendEvent::VenueLogoutDetected {
        venue: "KABU".to_string(),
    });

    assert_eq!(h.relogin_prompt().active.as_deref(), Some("KABU"));
}
