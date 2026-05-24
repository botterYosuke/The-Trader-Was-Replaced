//! M12 strategy_editor_hidden_in_manual — Strategy Editor は
//! `ExecutionMode::LiveManual` のときだけ隠れることを保証する（kind:ui / issue #31）。
//!
//! - フローティングウィンドウ root (`WindowRoot` + `PanelKind::StrategyEditor`) の
//!   `Visibility` と、サイドバーボタン (`Button` + `PanelKind::StrategyEditor`) の
//!   `Node.display` を `apply_strategy_editor_mode_visibility_system` が駆動する。
//! - Manual ONLY: Replay / LiveAuto では表示されたまま。
//! - Manual 中に新規 spawn されたウィンドウも隠れる（`is_changed()` ゲート無し）。
//! - 別 `PanelKind` のウィンドウは触らない。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes};
use backcast::ui::components::{PanelKind, WindowRoot};
use backcast::ui::strategy_editor::{
    apply_strategy_editor_mode_visibility_system, StrategyEditorModeHidden,
};

#[test]
fn m12_strategy_editor_hidden_in_manual() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<ExecutionModeRes>(); // 既定 = Replay
    app.add_systems(Update, apply_strategy_editor_mode_visibility_system);

    // Strategy Editor のフローティングウィンドウ root と、サイドバーボタンを用意。
    let window = app
        .world_mut()
        .spawn((WindowRoot, PanelKind::StrategyEditor, Visibility::Inherited))
        .id();
    let button = app
        .world_mut()
        .spawn((Button, PanelKind::StrategyEditor, Node::default()))
        .id();
    // 別 PanelKind のウィンドウ（Manual でも触られないこと）。
    let other = app
        .world_mut()
        .spawn((WindowRoot, PanelKind::BuyingPower, Visibility::Inherited))
        .id();

    // ── Replay（既定）→ 可視 / ボタン Flex ──
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(window).unwrap(),
        Visibility::Inherited,
        "Replay では Strategy Editor ウィンドウは可視のはず"
    );
    assert_eq!(
        app.world().get::<Node>(button).unwrap().display,
        Display::Flex,
        "Replay では Strategy Editor ボタンは表示されるはず"
    );

    // ── Manual → 非可視 / ボタン None ──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(window).unwrap(),
        Visibility::Hidden,
        "Manual では Strategy Editor ウィンドウは非可視のはず"
    );
    assert_eq!(
        app.world().get::<Node>(button).unwrap().display,
        Display::None,
        "Manual では Strategy Editor ボタンは隠れるはず"
    );
    // 退避マーカーが付いていること（Manual 中）。
    assert!(
        app.world().get::<StrategyEditorModeHidden>(window).is_some(),
        "Manual 中は退避マーカーが付いているはず"
    );
    // 別 PanelKind は Manual でも触られない。
    assert_eq!(
        *app.world().get::<Visibility>(other).unwrap(),
        Visibility::Inherited,
        "別 PanelKind のウィンドウは Manual でも触られないはず"
    );

    // ── Replay へ戻す → 元の可視性に復元 / ボタン Flex / マーカー除去 ──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(window).unwrap(),
        Visibility::Inherited,
        "Replay へ戻すと元の可視性（Inherited）に戻るはず"
    );
    assert_eq!(
        app.world().get::<Node>(button).unwrap().display,
        Display::Flex,
        "Replay へ戻すとボタンは再び表示されるはず"
    );
    assert!(
        app.world().get::<StrategyEditorModeHidden>(window).is_none(),
        "Manual を抜けたら退避マーカーは除去されるはず"
    );

    // ── LiveAuto → 表示されたまま（Manual only の回帰ガード）──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(window).unwrap(),
        Visibility::Inherited,
        "LiveAuto では Strategy Editor は表示されたままのはず（Manual only）"
    );
    assert_eq!(
        app.world().get::<Node>(button).unwrap().display,
        Display::Flex,
        "LiveAuto ではボタンは表示されたままのはず（Manual only）"
    );

    // ── Manual 中に新規 spawn したウィンドウも隠れる（is_changed ゲート無し）──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    app.update(); // 既存 window を隠す
    let late = app
        .world_mut()
        .spawn((WindowRoot, PanelKind::StrategyEditor, Visibility::Inherited))
        .id();
    app.update(); // Manual 中に spawn された late も捕捉して隠すはず
    assert_eq!(
        *app.world().get::<Visibility>(late).unwrap(),
        Visibility::Hidden,
        "Manual 中に spawn されたウィンドウも隠れるはず（is_changed ゲート無し）"
    );
}
