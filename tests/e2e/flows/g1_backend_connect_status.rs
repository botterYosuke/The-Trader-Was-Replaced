//! G1 backend_connect_status — backend 接続状態フラグ（footer の grpc: OK）。
//!
//! `Connected(true)` / `Running(true)` を受けると `BackendStatus` の connected /
//! running が立つことを確認する（footer の grpc 表示の根拠）。
//! 詳細は `tests/e2e/FLOWS.md` の G1 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn g1_backend_connect_status() {
    let mut h = Harness::new();
    assert!(!h.backend_connected());
    assert!(!h.backend_running());

    h.send_status(BackendStatusUpdate::Connected(true));
    h.send_status(BackendStatusUpdate::Running(true));

    assert!(h.backend_connected());
    assert!(h.backend_running());
}
