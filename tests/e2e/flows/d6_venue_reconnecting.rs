//! D6 venue_reconnecting — 接続中のネットワーク断が Reconnecting 表示になること。
//!
//! Connect → backend Connected で接続済みにした後、backend が
//! `VenueChanged{Reconnecting}` を押すと `VenueState::Reconnecting` になる
//! （network reconnect の表示）ことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D6 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn d6_venue_reconnecting() {
    let mut h = Harness::new();

    h.click(MenuItem::VenueConnectTachibanaDemo);
    h.drain_commands();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Reconnecting,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });
    assert_eq!(h.venue().state, VenueState::Reconnecting);
}
