//! D4 venue_logout — Venue→Disconnect 押下からログアウトで Disconnected に戻ること。
//!
//! Connect → backend が Connected を押した状態から、ユーザーが Venue→Disconnect を
//! 押すと本番 `menu_item_system` が `VenueLogout` を送る。その後 backend が
//! `VenueChanged{Disconnected}` を押し戻すと `VenueState::Disconnected` に戻ること
//! を確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn d4_venue_logout() {
    let mut h = Harness::new();

    // 接続まで進める（Connect → backend Connected）。
    h.click(MenuItem::VenueConnectTachibanaDemo);
    h.drain_commands();
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 10,
    });
    assert_eq!(h.venue().state, VenueState::Connected);

    // ユーザーが Venue→Disconnect を押す → VenueLogout コマンド。
    h.click(MenuItem::VenueDisconnect);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, TransportCommand::VenueLogout)),
        "Venue→Disconnect は VenueLogout を送るはず (got {cmds:?})"
    );

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Disconnected);
}
