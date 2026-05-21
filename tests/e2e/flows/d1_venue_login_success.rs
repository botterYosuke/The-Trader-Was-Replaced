//! D1 venue_login_success — Venue→Connect 押下からログイン成功までの状態遷移。
//!
//! ユーザーが Venue→Connect を押すと本番 `menu_item_system` が `VenueLogin` を送る。
//! その後 backend が `VenueChanged` を Disconnected → Authenticating → Connected と
//! 押し戻し、Connected 時に venue_id が記録されることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の D1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand, VenueState};
use backcast::ui::components::MenuItem;

#[test]
fn d1_venue_login_success() {
    let mut h = Harness::new();
    assert_eq!(h.venue().state, VenueState::Disconnected);

    // ユーザーが Venue→Connect (Tachibana demo) を押す → VenueLogin コマンド。
    h.click(MenuItem::VenueConnectTachibanaDemo);
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::VenueLogin { venue_id, .. } if venue_id == "tachibana"
        )),
        "Venue→Connect は VenueLogin を送るはず (got {cmds:?})"
    );

    // backend が状態遷移を押し戻す。
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
