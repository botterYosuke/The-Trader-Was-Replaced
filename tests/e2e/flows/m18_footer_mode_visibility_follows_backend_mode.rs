//! M18 footer_mode_visibility_follows_backend_mode — footer の
//! `apply_execution_mode_visibility_system` が **本番の mode writer（`status_update_system`）
//! 経由の backend mode 変化に同一フレームで追従する**ことを保証する（kind:state）。
//!
//! # 背景
//! `apply_execution_mode_visibility_system` は `is_changed()` ガード付きだが、
//! `.after(status_update_system)` 制約が無く、同フレームで前に走ると `is_changed()` が
//! false（mode がまだ書き換えられていない）で early-return し、1 フレーム遅延する。
//! （issue #41 codex review の Medium・footer.rs:809）
//!
//! M16 同様、本フローは `status_update_system` 経由に `ExecutionModeChanged` を流し、
//! mode 確定と同一 tick で `PauseResumeButton Node.display` が反映されることを保証する。
//! production の `apply_execution_mode_visibility_system.after(status_update_system)` と
//! 同じ順序制約をハーネス側にも張り、「mode 確定 → footer 可視性反映」が単一フレームで
//! 閉じる契約を固定する。

use bevy::prelude::*;

use crate::support::Harness;
use backcast::backend_sync::status_update_system;
use backcast::trading::{BackendStatusUpdate, ExecutionMode};
use backcast::ui::components::PauseResumeButton;
use backcast::ui::footer::apply_execution_mode_visibility_system;

#[test]
fn m18_footer_mode_visibility_follows_backend_mode() {
    let mut h = Harness::new();
    h.app.add_systems(
        Update,
        apply_execution_mode_visibility_system.after(status_update_system),
    );

    // PauseResumeButton: Replay / LiveAuto → Display::Flex、LiveManual → Display::None
    let pause = h
        .app
        .world_mut()
        .spawn((PauseResumeButton, Node::default()))
        .id();

    // 初期 Replay モード確定（is_changed が true になる最初の tick）
    h.tick();
    assert_eq!(
        h.app.world().get::<Node>(pause).unwrap().display,
        Display::Flex,
        "初期 Replay では PauseResumeButton は表示されるはず"
    );

    // backend が LiveManual を echo → 同一 tick で Display::None
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(
        h.exec_mode().mode,
        ExecutionMode::LiveManual,
        "前提: status_update_system が mode を LiveManual に確定する"
    );
    assert_eq!(
        h.app.world().get::<Node>(pause).unwrap().display,
        Display::None,
        "LiveManual echo と同一フレームで PauseResumeButton は非表示になるはず（1 フレーム遅延しない）"
    );

    // LiveAuto → 同一フレームで Display::Flex
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveAuto,
    });
    assert_eq!(
        h.app.world().get::<Node>(pause).unwrap().display,
        Display::Flex,
        "LiveAuto echo と同一フレームで PauseResumeButton は再表示されるはず"
    );

    // Replay へ戻す → 同一フレームで Display::Flex
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });
    assert_eq!(
        h.app.world().get::<Node>(pause).unwrap().display,
        Display::Flex,
        "Replay 復帰と同一フレームで PauseResumeButton は表示のはず"
    );
}
