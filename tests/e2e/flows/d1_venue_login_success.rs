//! D1 venue_login_success — venue ログイン成功の状態遷移。
//!
//! `VenueChanged` が `VenueState` を Disconnected → Authenticating → Connected
//! と駆動し、Connected 時に venue_id が記録されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, VenueState};

#[test]
fn d1_venue_login_success() {
    let mut h = Harness::new();
    assert_eq!(h.venue().state, VenueState::Disconnected);

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Authenticating,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Authenticating);

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    let v = h.venue();
    assert_eq!(v.state, VenueState::Connected);
    assert_eq!(v.venue_id.as_deref(), Some("tachibana"));
}
