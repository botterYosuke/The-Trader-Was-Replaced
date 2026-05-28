//! I10 open_live_switches_auto — Live モード中に File→Open を実行すると、
//! ダイアログ表示前に `SetExecutionMode(LiveAuto)` が送信されることを保証する（kind:ui）。
//!
//! # 駆動経路
//! Phase 8 §3.5.1 / §3.6.1 の seam:
//! `MenuItem::LoadLayout` を Pressed → 本番 `menu_item_system` が:
//!   - `ExecutionMode` が `LiveManual` / `LiveAuto` なら `SetExecutionMode(LiveAuto)` を先行送信
//!   - その後 `LayoutLoadDialogRequested` を発火
//!
//! # ケース
//! 1. LiveManual → Open: `SetExecutionMode(LiveAuto)` + `LayoutLoadDialogRequested`
//! 2. LiveAuto  → Open: `SetExecutionMode(LiveAuto)` + `LayoutLoadDialogRequested`
//! 3. Replay    → Open: `SetExecutionMode` は送信されず `LayoutLoadDialogRequested` のみ

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{ExecutionMode, ExecutionModeRes, TransportCommand, TransportCommandSender, VenueStatusRes};
use backcast::ui::components::{MenuItem, OpenMenu, PanelSpawnRequested, UndoMenuRequested, RedoMenuRequested};
use backcast::ui::layout_persistence::{
    LayoutLoadDialogRequested, LayoutSaveAsRequested, LayoutSaveRequested,
};
use backcast::ui::menu_bar::menu_item_system;

fn build_app(mode: ExecutionMode) -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(ExecutionModeRes { mode });
    app.insert_resource(VenueStatusRes::default());
    app.insert_resource(OpenMenu::default());
    app.insert_resource(TransportCommandSender { tx });

    app.add_message::<LayoutSaveRequested>();
    app.add_message::<LayoutSaveAsRequested>();
    app.add_message::<LayoutLoadDialogRequested>();
    app.add_message::<UndoMenuRequested>();
    app.add_message::<RedoMenuRequested>();
    app.add_message::<PanelSpawnRequested>();
    // issue #50 Step 0 spike — menu_item_system が SpikeEditorSpawnRequested を要求。Phase B で削除。
    app.add_message::<backcast::ui::strategy_editor_spike::SpikeEditorSpawnRequested>();

    app.add_systems(Update, menu_item_system);

    // LoadLayout ボタンを押した状態で注入。
    app.world_mut().spawn((
        Button,
        Interaction::Pressed,
        BackgroundColor::default(),
        MenuItem::LoadLayout,
    ));

    (app, rx)
}

#[test]
fn i10_open_live_switches_auto() {
    // ── ケース 1: LiveManual → Open ──
    {
        let (mut app, mut rx) = build_app(ExecutionMode::LiveManual);
        app.update();

        let cmd = rx
            .try_recv()
            .expect("LiveManual + Open で SetExecutionMode(LiveAuto) が送信されるはず");
        assert!(
            matches!(
                cmd,
                TransportCommand::SetExecutionMode {
                    mode: ExecutionMode::LiveAuto
                }
            ),
            "SetExecutionMode(LiveAuto) が先行送信されるはず、got {:?}",
            cmd
        );

        // ダイアログ要求イベントが発火したこと。
        let dialogs = app
            .world_mut()
            .resource_mut::<Messages<LayoutLoadDialogRequested>>()
            .drain()
            .count();
        assert_eq!(
            dialogs, 1,
            "LoadLayout ボタン押下で LayoutLoadDialogRequested が 1 回発火するはず"
        );

        // それ以上のコマンドは送信されない。
        assert!(
            rx.try_recv().is_err(),
            "SetExecutionMode 1 件以外のコマンドは送信されないはず"
        );
    }

    // ── ケース 2: LiveAuto → Open ──
    {
        let (mut app, mut rx) = build_app(ExecutionMode::LiveAuto);
        app.update();

        let cmd = rx
            .try_recv()
            .expect("LiveAuto + Open でも SetExecutionMode(LiveAuto) が送信されるはず");
        assert!(
            matches!(
                cmd,
                TransportCommand::SetExecutionMode {
                    mode: ExecutionMode::LiveAuto
                }
            ),
            "SetExecutionMode(LiveAuto) が先行送信されるはず、got {:?}",
            cmd
        );

        let dialogs = app
            .world_mut()
            .resource_mut::<Messages<LayoutLoadDialogRequested>>()
            .drain()
            .count();
        assert_eq!(dialogs, 1, "LayoutLoadDialogRequested が 1 回発火するはず");

        assert!(rx.try_recv().is_err(), "追加コマンドなし");
    }

    // ── ケース 3: Replay → Open: SetExecutionMode は送信されない ──
    {
        let (mut app, mut rx) = build_app(ExecutionMode::Replay);
        app.update();

        // コマンドなし（SetExecutionMode が送信されていないこと）。
        assert!(
            rx.try_recv().is_err(),
            "Replay モードの Open では SetExecutionMode が送信されないはず"
        );

        // ダイアログ要求は発火する。
        let dialogs = app
            .world_mut()
            .resource_mut::<Messages<LayoutLoadDialogRequested>>()
            .drain()
            .count();
        assert_eq!(
            dialogs, 1,
            "Replay モードでも LayoutLoadDialogRequested は 1 回発火するはず"
        );
    }
}
