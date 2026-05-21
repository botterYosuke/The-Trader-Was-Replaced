//! H5 exec_mode_change_resets_portfolio — モード切替で実モードが変わるとポートフォリオが
//! リセットされること。
//!
//! 口座データを入れた状態から、同一モードへの `ExecutionModeChanged` は no-op。ユーザーが
//! 実フッターの実行モードセグメントを押して `SetExecutionMode` を送り、backend が別モードへの
//! `ExecutionModeChanged` を押し戻すと `PortfolioState` が default にリセットされ、Live/Replay の
//! 口座データが混線しないことを確認する。口座データ混線防止の回帰の肝。
//! 詳細は `tests/e2e/FLOWS.md` の H5 を参照。

use crate::support::Harness;
use backcast::trading::{
    AccountPosition, BackendEvent, BackendStatusUpdate, ExecutionMode, TransportCommand, VenueState,
};
use backcast::ui::components::ExecutionModeToggleSegment;

#[test]
fn h5_exec_mode_change_resets_portfolio() {
    let mut h = Harness::new();
    // Populate account data.
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
    assert!(
        h.portfolio().loaded,
        "same-mode change is a no-op for the portfolio"
    );

    // ユーザーが Manual セグメントを押す → SetExecutionMode コマンド。
    h.set_venue(VenueState::Connected, "tachibana");
    h.click(ExecutionModeToggleSegment(ExecutionMode::LiveManual));
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::SetExecutionMode {
                mode: ExecutionMode::LiveManual
            }
        )),
        "モードトグルは SetExecutionMode を送るはず (got {cmds:?})"
    );

    // backend が実モード変更を押し戻す → ポートフォリオがリセットされる。
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    let p = h.portfolio();
    assert!(!p.loaded, "a real mode change resets the portfolio");
    assert!(p.positions.is_empty());
    assert_eq!(p.cash, 0.0);
}
