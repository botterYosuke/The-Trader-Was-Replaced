//! D3 venue_login_error — Connect 押下後のログイン失敗が Error として現れること。
//!
//! ユーザーが Venue→Connect を押すと本番 `menu_item_system` が `VenueLogin` を送る。
//! その後 backend が `VenueChanged{Error}` を押し戻すと `VenueState::Error` に遷移する
//! ことを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D3 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn d3_venue_login_error() {
    let mut h = Harness::new();

    h.click(MenuItem::VenueConnectTachibanaProd);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::VenueLogin { venue_id, .. } if venue_id == "tachibana"
        )),
        "Venue→Connect は VenueLogin を送るはず (got {cmds:?})"
    );

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Error,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.venue().state, VenueState::Error);
}
