//! A2 replay_pause_resume — 一時停止中はリプレイ時計が止まり、再開で進むこと。
//!
//! UI は backend が押し出すリプレイ時計をミラーする。新しい state が来ない
//! （pause）間は tick しても `TradingSession.timestamp_ms` が進まず、後続の
//! push_state（resume）で進む。最後に `RunComplete` で Completed になる。
//! gRPC 経由の Pause/Resume は transport task 依存（Phase A-full）のため、ここ
//! では「時計のミラー忠実性」のみを検証する。
//! 詳細は `tests/e2e/FLOWS.md` の A2 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn a2_replay_pause_resume() {
    let mut h = Harness::new();

    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    h.push_state(1000);
    assert_eq!(h.timestamp_ms(), 1000);

    // Pause: no new state pushed, ticking must not advance the clock.
    h.tick();
    h.tick();
    assert_eq!(h.timestamp_ms(), 1000);

    // Resume: backend pushes a later clock, UI mirrors it.
    h.push_state(2000);
    assert_eq!(h.timestamp_ms(), 2000);

    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-2".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}
