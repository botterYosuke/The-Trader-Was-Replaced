//! A3 replay_step_forward — ステップ実行で時計が1単位ずつ前進すること。
//!
//! backend が1ステップごとに押し出す state を受けるたびに
//! `TradingSession.timestamp_ms` が1進む。StepForward 自体は gRPC コマンド
//! （transport task 依存）のため、ここでは backend が押し戻す per-step 時計を
//! UI が忠実にミラーすることを検証する。
//! 詳細は `tests/e2e/FLOWS.md` の A3 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn a3_replay_step_forward() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);

    for step in 1..=4i64 {
        h.push_state(step);
        assert_eq!(h.timestamp_ms(), step);
    }
}
