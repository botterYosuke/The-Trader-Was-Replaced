//! I4 mode_toggle_client_gating — 実行モードトグルは前提条件を満たすときだけ
//! `SetExecutionMode` を backend へ送信し、満たさないときはローカル状態も command も変えないことを保証する（kind:ui）。
//!
//! テストでは Replay / Manual / Auto segment click を注入し、`TransportCommandSender` の送信有無と `ExecutionModeRes` を観測する。
//! `execution_mode_toggle_system` (footer.rs ~657) の precondition:
//! - Live への遷移: venue が Disconnected / Error なら blocked。
//! - Replay への遷移: 常に許可（ホームモード）。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    ExecutionMode, ExecutionModeRes, TransportCommand, TransportCommandSender, VenueState,
    VenueStatusRes,
};
use backcast::ui::components::{ExecutionModeToggleSegment, StrategyBuffer};
use backcast::ui::footer::execution_mode_toggle_system;

/// テスト用 App を共通構築し、TransportCommand の受信端を返す。
fn build_app(
    current_mode: ExecutionMode,
    venue_state: VenueState,
    strategy_loaded: bool,
) -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(ExecutionModeRes { mode: current_mode });
    app.insert_resource(VenueStatusRes {
        state: venue_state,
        ..Default::default()
    });
    app.insert_resource(StrategyBuffer {
        original_path: if strategy_loaded {
            Some(std::path::PathBuf::from("/tmp/strat.py"))
        } else {
            None
        },
        ..Default::default()
    });
    app.insert_resource(TransportCommandSender { tx });
    app.add_systems(Update, execution_mode_toggle_system);

    (app, rx)
}

/// segment ボタンを spawn して 1 フレーム回す。
fn press_segment(app: &mut App, target: ExecutionMode) {
    app.world_mut().spawn((
        Button,
        Interaction::Pressed,
        ExecutionModeToggleSegment(target),
    ));
    app.update();
}

#[test]
fn i4_mode_toggle_client_gating() {
    // ── ケース 1: LiveManual へ遷移 — venue が Disconnected → blocked ──
    // 送信なし。
    {
        let (mut app, mut rx) =
            build_app(ExecutionMode::Replay, VenueState::Disconnected, true);
        press_segment(&mut app, ExecutionMode::LiveManual);

        assert!(
            rx.try_recv().is_err(),
            "venue Disconnected で Live へ遷移しようとしても SetExecutionMode は送られないはず"
        );
        // optimistic update なし（ExecutionModeRes は backend diff でのみ更新される）
        assert_eq!(
            app.world().resource::<ExecutionModeRes>().mode,
            ExecutionMode::Replay,
            "precondition NG でも ExecutionModeRes はローカルで変わらないはず"
        );
    }

    // ── ケース 2: LiveAuto へ遷移 — venue が Error → blocked ──
    {
        let (mut app, mut rx) =
            build_app(ExecutionMode::Replay, VenueState::Error, true);
        press_segment(&mut app, ExecutionMode::LiveAuto);

        assert!(
            rx.try_recv().is_err(),
            "venue Error で LiveAuto へ遷移しようとしても SetExecutionMode は送られないはず"
        );
    }

    // ── ケース 3: Replay へ遷移 — strategy 未ロードでも常に送信（Replay はホームモード）──
    {
        let (mut app, mut rx) =
            build_app(ExecutionMode::LiveManual, VenueState::Connected, false);
        press_segment(&mut app, ExecutionMode::Replay);

        let cmd = rx
            .try_recv()
            .expect("strategy 未ロードでも Replay へ遷移すると SetExecutionMode が送られるはず");
        assert!(
            matches!(
                cmd,
                TransportCommand::SetExecutionMode {
                    mode: ExecutionMode::Replay
                }
            ),
            "SetExecutionMode(Replay) が送られるはず、got {:?}",
            cmd
        );
    }

    // ── ケース 4: 同じモードへのクリック → no-op（重複送信なし）──
    {
        let (mut app, mut rx) =
            build_app(ExecutionMode::Replay, VenueState::Connected, true);
        // 現在 Replay → Replay クリックは exec_mode.mode == target で continue する
        press_segment(&mut app, ExecutionMode::Replay);

        assert!(
            rx.try_recv().is_err(),
            "現在と同じモードへのクリックは SetExecutionMode を送らないはず"
        );
    }

    // ── ケース 5: Live への遷移 — venue が Connected + strategy ロード済み → 送信 ──
    {
        let (mut app, mut rx) =
            build_app(ExecutionMode::Replay, VenueState::Connected, true);
        press_segment(&mut app, ExecutionMode::LiveManual);

        let cmd = rx
            .try_recv()
            .expect("venue Connected で Live へ遷移すると SetExecutionMode が送られるはず");
        assert!(
            matches!(
                cmd,
                TransportCommand::SetExecutionMode {
                    mode: ExecutionMode::LiveManual
                }
            ),
            "SetExecutionMode(LiveManual) が送られるはず、got {:?}",
            cmd
        );
    }

    // ── ケース 6: Replay への遷移 — strategy ロード済み → 送信 ──
    {
        let (mut app, mut rx) =
            build_app(ExecutionMode::LiveManual, VenueState::Disconnected, true);
        press_segment(&mut app, ExecutionMode::Replay);

        let cmd = rx
            .try_recv()
            .expect("strategy ロード済みで Replay へ遷移すると SetExecutionMode が送られるはず");
        assert!(
            matches!(
                cmd,
                TransportCommand::SetExecutionMode {
                    mode: ExecutionMode::Replay
                }
            ),
            "SetExecutionMode(Replay) が送られるはず、got {:?}",
            cmd
        );
    }
}
