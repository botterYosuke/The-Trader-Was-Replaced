//! D9 venue_stays_connected_on_replay_toggle — Replay モード切替は VenueChanged を伴わない契約。
//!
//! venue が Connected の状態で backend が `ExecutionModeChanged{Replay}` を送ってきたとき、
//! `VenueChanged{Disconnected}` を伴わなければ venue は Connected のままであることを保証する。
//!
//! issue #39 の Rust フロント側の契約テスト:
//!   venue Connected → ExecutionModeChanged{Replay} を VenueChanged{Disconnected} なしで注入
//!   → venue が Connected のまま、を assert する。
//!
//! 詳細は `tests/e2e/FLOWS.md` の D9 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, ExecutionMode, VenueState};

#[test]
fn d9_venue_stays_connected_on_replay_toggle() {
    let mut h = Harness::new();

    // 前提: venue を Connected 状態にしておく。
    h.set_venue(VenueState::Connected, "tachibana");
    assert_eq!(h.venue().state, VenueState::Connected);

    // backend が ExecutionModeChanged{Replay} を送る（VenueChanged は伴わない）。
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });

    // venue は Connected のまま: Replay 切替は venue を切断しない契約。
    assert_eq!(
        h.venue().state,
        VenueState::Connected,
        "ExecutionModeChanged{{Replay}} だけでは venue は Disconnected にならない"
    );
}
