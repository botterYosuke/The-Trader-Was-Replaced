//! E1 set_execution_mode — モードトグル押下は `SetExecutionMode` を送るだけで、
//! 実モードは backend 権威で反映されること。
//!
//! 実フッターの実行モードセグメントを本番 `execution_mode_toggle_system` で駆動する。
//! venue 接続済みで Manual を押すと `TransportCommand::SetExecutionMode{LiveManual}` を
//! 送るが、UI は `ExecutionModeRes` を optimistic に書き換えない（backend desync 防止）。
//! その後 backend が `ExecutionModeChanged{LiveManual}` を押し戻して初めてモードが変わる。
//! 詳細は `tests/e2e/FLOWS.md` の E1 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, ExecutionMode, TransportCommand, VenueState};
use backcast::ui::components::ExecutionModeToggleSegment;

#[test]
fn e1_set_execution_mode() {
    let mut h = Harness::new();
    assert_eq!(h.exec_mode().mode, ExecutionMode::Replay);

    // Live への遷移 precondition: venue 接続。
    h.set_venue(VenueState::Connected, "tachibana");

    // ユーザーが Manual セグメントを押す → SetExecutionMode コマンド。
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
    // UI は optimistic に書き換えない: backend 確定まで Replay のまま。
    assert_eq!(
        h.exec_mode().mode,
        ExecutionMode::Replay,
        "mode は backend authoritative（optimistic 更新しない）"
    );

    // backend が確定を押し戻す。
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(h.exec_mode().mode, ExecutionMode::LiveManual);
}

#[test]
fn e1_replay_live_manual_replay_round_trip() {
    // Replay → LiveManual → Replay の完全往復。
    // Replay への帰還は strategy 未ロード・replay IDLE でも常に成功する（ホームモード）。
    let mut h = Harness::new();
    assert_eq!(h.exec_mode().mode, ExecutionMode::Replay);

    h.set_venue(VenueState::Connected, "tachibana");

    // ── 1. Replay → LiveManual ──
    h.click(ExecutionModeToggleSegment(ExecutionMode::LiveManual));
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::SetExecutionMode {
                mode: ExecutionMode::LiveManual
            }
        )),
        "Replay→LiveManual: SetExecutionMode(LiveManual) が送られるはず (got {cmds:?})"
    );
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(h.exec_mode().mode, ExecutionMode::LiveManual);

    // ── 2. LiveManual → Replay（strategy 未ロード、replay IDLE のまま）──
    h.click(ExecutionModeToggleSegment(ExecutionMode::Replay));
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::SetExecutionMode {
                mode: ExecutionMode::Replay
            }
        )),
        "LiveManual→Replay: strategy 未ロード・IDLE でも SetExecutionMode(Replay) が送られるはず (got {cmds:?})"
    );
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });
    assert_eq!(h.exec_mode().mode, ExecutionMode::Replay);
}
