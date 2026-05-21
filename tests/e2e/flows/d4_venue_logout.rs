//! D4 venue_logout — ログアウトで Disconnected に戻ること。
//!
//! Connected の状態から `VenueChanged{Disconnected}` を受けると
//! `VenueState::Disconnected` に戻ることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, VenueState};

#[test]
fn d4_venue_logout() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });
    assert_eq!(h.venue().state, VenueState::Connected);

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Disconnected);
}
