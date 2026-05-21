//! D3 venue_login_error — ログイン失敗が Error として現れること。
//!
//! `VenueChanged{Error}` で `VenueState::Error` に遷移することを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D3 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, VenueState};

#[test]
fn d3_venue_login_error() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Error,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Error);
}
