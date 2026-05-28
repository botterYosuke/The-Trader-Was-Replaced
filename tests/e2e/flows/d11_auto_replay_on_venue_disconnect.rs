//! D11 auto_replay_on_venue_disconnect — Venue が切断されたとき、Manual / Auto
//! モード中なら自動で Replay に切り替わることを保証する（kind:ui）。
//!
//! # 仕様
//! - `VenueState` が非 live（Disconnected / Reconnecting / Error 等）に遷移したとき
//!   `ExecutionModeRes.mode` が `LiveManual` または `LiveAuto` なら、同一フレームで
//!   `Replay` に自動切り替わる。
//! - 既に `Replay` の場合は変化しない。
//! - `VenueState` が live（Connected / Subscribed）に遷移しても mode は変化しない。
//!
//! `auto_replay_on_venue_disconnect_system` の回帰ガード。

use bevy::prelude::*;

use crate::support::Harness;
use backcast::backend_sync::status_update_system;
use backcast::trading::{BackendStatusUpdate, ExecutionMode, ExecutionModeRes, VenueState};
use backcast::ui::footer::auto_replay_on_venue_disconnect_system;

fn setup_harness() -> Harness {
    let mut h = Harness::new();
    h.app.add_systems(
        Update,
        auto_replay_on_venue_disconnect_system.after(status_update_system),
    );
    h
}

#[test]
fn d11_manual_mode_reverts_to_replay_on_disconnect() {
    let mut h = setup_harness();
    // LiveManual にセット
    h.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    h.tick();

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::Replay,
        "Venue 切断時: LiveManual → Replay に自動切り替わるはず"
    );
}

#[test]
fn d11_auto_mode_reverts_to_replay_on_disconnect() {
    let mut h = setup_harness();
    h.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    h.tick();

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::Replay,
        "Venue 切断時: LiveAuto → Replay に自動切り替わるはず"
    );
}

#[test]
fn d11_manual_mode_reverts_to_replay_on_reconnecting() {
    let mut h = setup_harness();
    h.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    h.tick();

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Reconnecting,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::Replay,
        "Reconnecting 時: LiveManual → Replay に自動切り替わるはず"
    );
}

#[test]
fn d11_replay_mode_unchanged_on_disconnect() {
    let mut h = setup_harness();
    // すでに Replay → 変化しない
    assert_eq!(
        h.app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::Replay
    );
    h.tick();

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::Replay,
        "すでに Replay: Disconnect しても変化しないはず"
    );
}

#[test]
fn d11_mode_unchanged_on_venue_connected() {
    let mut h = setup_harness();
    // 初期フレーム: Replay + Disconnected → auto-switch 不発（Replay は変化不要）
    h.tick();

    // Connected に遷移してから LiveManual に（ユーザーが手動切替した想定）
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    h.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;

    // さらに live 遷移（Connected → Subscribed）が来ても mode は変化しない
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Subscribed,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 3,
    });

    assert_eq!(
        h.app.world().resource::<ExecutionModeRes>().mode,
        ExecutionMode::LiveManual,
        "live → live 遷移では mode を変えないはず"
    );
}
