//! D2 venue_subscribed — 購読開始で Subscribed になり銘柄数が反映されること。
//!
//! `VenueChanged{Subscribed, instruments_loaded}` で `VenueState::Subscribed`
//! になり、購読できた銘柄数 `instruments_loaded` が反映されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, VenueState};

#[test]
fn d2_venue_subscribed() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Subscribed,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 128,
    });
    let v = h.venue();
    assert_eq!(v.state, VenueState::Subscribed);
    assert_eq!(v.instruments_loaded, 128);
}
