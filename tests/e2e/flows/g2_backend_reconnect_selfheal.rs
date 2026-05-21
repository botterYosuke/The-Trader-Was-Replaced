//! G2 backend_reconnect_selfheal — 接続断からの自己修復（latch off しない）。
//!
//! `Error` で connected=false になり last_error が記録され、その後の
//! `Connected(true)` で接続が復旧することを確認する。接続状態が落ちたまま
//! 固着しない（self-heal）ことの回帰ガード。
//! 詳細は `tests/e2e/FLOWS.md` の G2 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn g2_backend_reconnect_selfheal() {
    let mut h = Harness::new();
    h.send_status(BackendStatusUpdate::Connected(true));
    assert!(h.backend_connected());

    // Connection drops.
    h.send_status(BackendStatusUpdate::Error("stream reset".to_string()));
    assert!(!h.backend_connected(), "Error must drop the connection");
    assert_eq!(h.backend_last_error().as_deref(), Some("stream reset"));

    // Recovery: a fresh Connected(true) re-establishes the link.
    h.send_status(BackendStatusUpdate::Connected(true));
    assert!(h.backend_connected(), "connection must self-heal, not latch off");
}
