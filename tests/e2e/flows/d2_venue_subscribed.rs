//! D2 venue_subscribed — Connect 後、購読開始で Subscribed になり銘柄数が反映されること。
//!
//! ユーザーが Venue→Connect を押すと本番 `menu_item_system` が `VenueLogin` を送る。
//! その後 backend が `VenueChanged{Subscribed, instruments_loaded}` を押すと
//! `VenueState::Subscribed` になり、購読できた銘柄数が反映されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn d2_venue_subscribed() {
    let mut h = Harness::new();

    h.click(MenuItem::VenueConnectTachibanaDemo);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::VenueLogin { venue_id, .. } if venue_id == "tachibana"
        )),
        "Venue→Connect は VenueLogin を送るはず (got {cmds:?})"
    );

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Subscribed,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 128,
    });
    let v = h.venue();
    assert_eq!(v.state, VenueState::Subscribed);
    assert_eq!(v.instruments_loaded, 128);
}
