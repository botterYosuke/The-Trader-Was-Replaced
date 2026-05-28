//! D10 venue_live_buttons_visibility — Venue 接続状態に応じて Manual / Auto ボタンが
//! 表示・非表示になることを保証する（kind:ui）。
//!
//! # 仕様
//! - 起動直後（Disconnected）: Manual / Auto ボタンは `Display::None`
//! - `VenueState::Connected` / `Subscribed`: 表示（`Display::Flex`）
//! - `Disconnected` / `Reconnecting` 等に戻ると再び `Display::None`
//! - Replay ボタンは venue 状態によらず常に表示
//!
//! `apply_venue_live_button_visibility_system` が `VenueStatusRes` 変化に同一フレームで
//! 追従する契約を固定する（issue #55 回帰ガード）。

use bevy::prelude::*;

use crate::support::Harness;
use backcast::backend_sync::status_update_system;
use backcast::trading::{BackendStatusUpdate, VenueState};
use backcast::ui::components::ExecutionModeToggleSegment;
use backcast::trading::ExecutionMode;
use backcast::ui::footer::apply_venue_live_button_visibility_system;

fn setup_harness() -> (Harness, Entity, Entity, Entity) {
    let mut h = Harness::new();
    h.app.add_systems(
        Update,
        apply_venue_live_button_visibility_system.after(status_update_system),
    );

    let replay = h
        .app
        .world_mut()
        .spawn((ExecutionModeToggleSegment(ExecutionMode::Replay), Node::default(), Button))
        .id();
    let manual = h
        .app
        .world_mut()
        .spawn((ExecutionModeToggleSegment(ExecutionMode::LiveManual), Node::default(), Button))
        .id();
    let auto_btn = h
        .app
        .world_mut()
        .spawn((ExecutionModeToggleSegment(ExecutionMode::LiveAuto), Node::default(), Button))
        .id();

    (h, replay, manual, auto_btn)
}

#[test]
fn d10_venue_live_buttons_hidden_on_startup() {
    let (mut h, replay, manual, auto_btn) = setup_harness();
    // 初期 Disconnected 状態で最初の tick を走らせる。
    h.tick();

    assert_eq!(
        h.app.world().get::<Node>(manual).unwrap().display,
        Display::None,
        "起動直後（Disconnected）: Manual ボタンは非表示のはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(auto_btn).unwrap().display,
        Display::None,
        "起動直後（Disconnected）: Auto ボタンは非表示のはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(replay).unwrap().display,
        Display::Flex,
        "起動直後: Replay ボタンは表示のはず（display は変更されない）"
    );
}

#[test]
fn d10_venue_live_buttons_show_on_connected() {
    let (mut h, _replay, manual, auto_btn) = setup_harness();
    h.tick(); // 初期フレーム

    // backend が Connected を通知 → 同一 tick で表示
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().get::<Node>(manual).unwrap().display,
        Display::Flex,
        "Connected: Manual ボタンは表示されるはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(auto_btn).unwrap().display,
        Display::Flex,
        "Connected: Auto ボタンは表示されるはず"
    );
}

#[test]
fn d10_venue_live_buttons_show_on_subscribed() {
    let (mut h, _replay, manual, auto_btn) = setup_harness();
    h.tick();

    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Subscribed,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 3,
    });

    assert_eq!(
        h.app.world().get::<Node>(manual).unwrap().display,
        Display::Flex,
        "Subscribed: Manual ボタンは表示されるはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(auto_btn).unwrap().display,
        Display::Flex,
        "Subscribed: Auto ボタンは表示されるはず"
    );
}

#[test]
fn d10_venue_live_buttons_rehide_on_disconnect() {
    let (mut h, _replay, manual, auto_btn) = setup_harness();
    h.tick();

    // Connected にして表示させる
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.app.world().get::<Node>(manual).unwrap().display, Display::Flex);

    // 切断 → 同一 tick で非表示に戻る
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Disconnected,
        venue_id: None,
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().get::<Node>(manual).unwrap().display,
        Display::None,
        "Disconnected: Manual ボタンは再び非表示になるはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(auto_btn).unwrap().display,
        Display::None,
        "Disconnected: Auto ボタンは再び非表示になるはず"
    );
}

#[test]
fn d10_venue_live_buttons_hidden_on_reconnecting() {
    let (mut h, _replay, manual, auto_btn) = setup_harness();
    h.tick();

    // Connected にして表示させる
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(h.app.world().get::<Node>(manual).unwrap().display, Display::Flex);

    // Reconnecting に遷移 → 非表示に戻る（is_venue_live は false）
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Reconnecting,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });

    assert_eq!(
        h.app.world().get::<Node>(manual).unwrap().display,
        Display::None,
        "Reconnecting: Manual ボタンは非表示になるはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(auto_btn).unwrap().display,
        Display::None,
        "Reconnecting: Auto ボタンは非表示になるはず"
    );
}

#[test]
fn d10_replay_button_unaffected_by_venue_state() {
    let (mut h, replay, _manual, _auto_btn) = setup_harness();
    h.tick();

    // Disconnected のまま
    assert_eq!(
        h.app.world().get::<Node>(replay).unwrap().display,
        Display::Flex,
        "Disconnected 時: Replay ボタンの display は変更されないはず"
    );

    // Connected になっても Replay ボタンは触らない
    h.send_status(BackendStatusUpdate::VenueChanged {
        state: VenueState::Connected,
        venue_id: Some("tachibana".to_string()),
        instruments_loaded: 0,
    });
    assert_eq!(
        h.app.world().get::<Node>(replay).unwrap().display,
        Display::Flex,
        "Connected 後も Replay ボタンの display は変更されないはず"
    );
}
