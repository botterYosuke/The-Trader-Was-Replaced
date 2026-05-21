//! H5 exec_mode_change_resets_portfolio — 実モード変更でポートフォリオがリセットされること。
//!
//! 実際の `ExecutionModeChanged` で `PortfolioState` が default にリセットされ、
//! Live/Replay の口座データが混線しないようにする。同一モードへの変更は no-op。
//! 口座データ混線防止の回帰の肝。
//! 詳細は `tests/e2e/FLOWS.md` の H5 を参照。

use crate::support::Harness;
use backcast::trading::{AccountPosition, BackendEvent, BackendStatusUpdate, ExecutionMode};

#[test]
fn h5_exec_mode_change_resets_portfolio() {
    let mut h = Harness::new();
    // Populate Live account data.
    h.send_event(BackendEvent::AccountEvent {
        cash: 50_000.0,
        buying_power: 120_000.0,
        positions: vec![AccountPosition {
            symbol: "1301.TSE".to_string(),
            qty: 100,
            avg_price: 1500.0,
            unrealized_pnl: 0.0,
        }],
        ts_ms: 1000,
    });
    assert!(h.portfolio().loaded);

    // A no-op change (already Replay) must not wipe the data.
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });
    assert!(h.portfolio().loaded, "same-mode change is a no-op for the portfolio");

    // A real change wipes the mode-specific account snapshot.
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    let p = h.portfolio();
    assert!(!p.loaded, "a real mode change resets the portfolio");
    assert!(p.positions.is_empty());
    assert_eq!(p.cash, 0.0);
}
