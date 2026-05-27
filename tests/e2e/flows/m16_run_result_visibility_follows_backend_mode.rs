//! M16 run_result_visibility_follows_backend_mode — RUN RESULT の可視性が
//! **本番の mode writer（`status_update_system`）経由の backend mode 変化に
//! 同一フレームで追従する**ことを保証する（kind:state）。
//!
//! M14 は `ExecutionModeRes` を直接書き換えるので、可視性 system が本番で
//! `status_update_system` より前に走ると 1 フレーム古い可視性を出す bug
//! （issue #41 codex review の Medium）を捕捉できない。本フローは
//! `BackendStatusUpdate::ExecutionModeChanged` を実 channel に流し、
//! `status_update_system` が `ExecutionModeRes` を確定した**同じ tick** で
//! RUN RESULT の `Visibility` が正しい値になることを assert する。
//!
//! production の `apply_run_result_visibility_system.after(status_update_system)`
//! と同じ順序制約をハーネス側にも張り、「mode 確定 → 可視性反映」が単一フレームで
//! 閉じる契約を固定する。

use bevy::prelude::*;

use crate::support::Harness;
use backcast::backend_sync::status_update_system;
use backcast::trading::{BackendStatusUpdate, ExecutionMode};
use backcast::ui::components::RunResultPanelRoot;
use backcast::ui::run_result_panel::apply_run_result_visibility_system;

#[test]
fn m16_run_result_visibility_follows_backend_mode() {
    let mut h = Harness::new();
    // 本番（src/ui/mod.rs）と同じ順序制約で可視性 system を載せる。
    h.app.add_systems(
        Update,
        apply_run_result_visibility_system.after(status_update_system),
    );
    let root = h
        .app
        .world_mut()
        .spawn((RunResultPanelRoot, Visibility::Inherited))
        .id();

    // 既定は Replay → 可視のまま。
    h.tick();
    assert_eq!(
        *h.app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "既定 Replay では RUN RESULT は可視のはず"
    );

    // backend が LiveManual を echo。send_status は push 後に 1 tick だけ進めるので、
    // status_update_system が mode を確定した同一フレームで Hidden になっていること。
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(
        h.exec_mode().mode,
        ExecutionMode::LiveManual,
        "前提: status_update_system が mode を LiveManual に確定する"
    );
    assert_eq!(
        *h.app.world().get::<Visibility>(root).unwrap(),
        Visibility::Hidden,
        "LiveManual を echo した同一フレームで RUN RESULT は隠れるはず（1 フレーム遅延しない）"
    );

    // LiveAuto → 同一フレームで可視。
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveAuto,
    });
    assert_eq!(
        *h.app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "LiveAuto を echo した同一フレームで RUN RESULT は再表示されるはず"
    );

    // Replay へ戻す → 同一フレームで可視。
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });
    assert_eq!(
        *h.app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited,
        "Replay へ戻した同一フレームで RUN RESULT は可視のはず"
    );
}
