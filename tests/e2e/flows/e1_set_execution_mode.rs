//! E1 set_execution_mode — 実行モード切替が backend 権威で反映されること。
//!
//! `ExecutionModeChanged{LiveManual}` で `ExecutionModeRes.mode` が LiveManual
//! になることを確認する（モードは backend が authoritative）。
//! 詳細は `tests/e2e/FLOWS.md` の E1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, ExecutionMode};

#[test]
fn e1_set_execution_mode() {
    let mut h = Harness::new();
    assert_eq!(h.exec_mode().mode, ExecutionMode::Replay);

    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(h.exec_mode().mode, ExecutionMode::LiveManual);
}
