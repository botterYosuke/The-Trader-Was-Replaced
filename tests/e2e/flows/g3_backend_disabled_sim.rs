//! G3 backend_disabled_sim — backend 無効時はリプレイ時計を進めないこと。
//!
//! `backend_enabled = false`（footer は grpc: DISABLED 表示）のとき
//! `backend_update_system` が early-return し、push した時計が no-op
//! （`TradingSession.timestamp_ms` 不変）であることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の G3 を参照。

use crate::support::Harness;

#[test]
fn g3_backend_disabled_sim() {
    let mut h = Harness::new_backend_disabled();
    assert!(!h.backend_enabled(), "footer would render grpc: DISABLED");
    assert_eq!(h.timestamp_ms(), 0);

    // backend_update_system early-returns: the pushed clock must be ignored.
    h.push_state(5000);
    assert_eq!(h.timestamp_ms(), 0, "disabled backend must not advance the clock");
}
