//! I6 file_new_resets_loaded_strategy — File → New がロード済み戦略・シナリオ・パネル状態を破棄し、
//! 新規状態（Live Manual）へ戻すことを保証する（kind:ui）。
//!
//! # 駆動経路
//! `MenuItem::FileNew` に `Interaction::Pressed` を注入 → 本番 `menu_item_system` が:
//!   1. `ForceStop` を送信
//!   2. `SetExecutionMode(LiveManual)` を送信
//!
//! # 観測
//! - `TransportCommandSender` 経由で送信されたコマンドの順番と種類
//! - `TransportCommandSender` が無い場合は silent drop（パニックしない）
//!
//! # 注意
//! menu_item_system のコメント (Phase 8 §3.5 / §3.6) によると、StrategyBuffer の
//! original_path クリア等は「別 Step に繰り越す」とあるため、現実装では
//! ForceStop + SetExecutionMode の送信だけをアサートする。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{ExecutionMode, ExecutionModeRes, TransportCommand, TransportCommandSender, VenueStatusRes};
use backcast::ui::components::{MenuItem, OpenMenu};
use backcast::ui::layout_persistence::{
    LayoutLoadDialogRequested, LayoutSaveAsRequested, LayoutSaveRequested,
};
use backcast::ui::menu_bar::menu_item_system;
use backcast::ui::components::{UndoMenuRequested, RedoMenuRequested};

#[test]
fn i6_file_new_resets_loaded_strategy() {
    // ── セットアップ ──
    let (tx, mut rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(ExecutionModeRes { mode: ExecutionMode::Replay });
    app.insert_resource(VenueStatusRes::default());
    app.insert_resource(OpenMenu::default());
    app.insert_resource(TransportCommandSender { tx });

    app.add_event::<LayoutSaveRequested>();
    app.add_event::<LayoutSaveAsRequested>();
    app.add_event::<LayoutLoadDialogRequested>();
    app.add_event::<UndoMenuRequested>();
    app.add_event::<RedoMenuRequested>();

    app.add_systems(Update, menu_item_system);

    // FileNew ボタンを `Interaction::Pressed` で注入。
    // BackgroundColor が必要 (menu_item_system は &mut BackgroundColor を query する)。
    app.world_mut().spawn((
        Button,
        Interaction::Pressed,
        BackgroundColor::default(),
        MenuItem::FileNew,
    ));
    app.update();

    // ── コマンド検証 ──
    // Phase 8 §3.5 / §3.6: ForceStop が先、その後 SetExecutionMode(LiveManual)。
    let cmd1 = rx.try_recv().expect("ForceStop が送信されるはず");
    assert!(
        matches!(cmd1, TransportCommand::ForceStop),
        "1 番目のコマンドは ForceStop のはず、got {:?}",
        cmd1
    );

    let cmd2 = rx.try_recv().expect("SetExecutionMode(LiveManual) が送信されるはず");
    assert!(
        matches!(
            cmd2,
            TransportCommand::SetExecutionMode {
                mode: ExecutionMode::LiveManual
            }
        ),
        "2 番目のコマンドは SetExecutionMode(LiveManual) のはず、got {:?}",
        cmd2
    );

    // それ以上のコマンドは送信されない。
    assert!(
        rx.try_recv().is_err(),
        "FileNew で ForceStop + SetExecutionMode 以外のコマンドは送信されないはず"
    );

    // ── Sender 無しの場合は silent drop（パニックしない）──
    {
        let mut app2 = App::new();
        app2.insert_resource(ExecutionModeRes { mode: ExecutionMode::Replay });
        app2.insert_resource(VenueStatusRes::default());
        app2.insert_resource(OpenMenu::default());
        // TransportCommandSender を挿入しない → Option<Res<>> は None

        app2.add_event::<LayoutSaveRequested>();
        app2.add_event::<LayoutSaveAsRequested>();
        app2.add_event::<LayoutLoadDialogRequested>();
        app2.add_event::<UndoMenuRequested>();
        app2.add_event::<RedoMenuRequested>();

        app2.add_systems(Update, menu_item_system);

        app2.world_mut().spawn((
            Button,
            Interaction::Pressed,
            BackgroundColor::default(),
            MenuItem::FileNew,
        ));
        // Sender なしでも panic しない
        app2.update();
    }
}
