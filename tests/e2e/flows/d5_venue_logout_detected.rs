//! D5 venue_logout_detected — 外部ログアウト検知で再ログインモーダルが開くこと。
//!
//! `BackendEvent::VenueLogoutDetected{venue}` で `ReloginPrompt.active` が
//! `Some(venue)` になり、venue が落ちたことをユーザーに通知する（Phase 9 Step 7
//! の health watchdog で実装）。モーダルは通知に徹し自身は再ログインしない
//! （再ログインは Venue メニュー経由）ため、`VenueStatusRes` は意図的に不変。
//! 詳細は `tests/e2e/FLOWS.md` の D5 を参照。

use crate::support::Harness;
use backcast::trading::BackendEvent;

#[test]
fn d5_venue_logout_detected() {
    let mut h = Harness::new();
    assert!(h.relogin_prompt().active.is_none());

    h.send_event(BackendEvent::VenueLogoutDetected {
        venue: "KABU".to_string(),
    });

    assert_eq!(h.relogin_prompt().active.as_deref(), Some("KABU"));
}
