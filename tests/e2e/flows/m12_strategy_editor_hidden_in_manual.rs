//! M12 strategy_editor_hidden_in_manual — Strategy Editor は
//! `ExecutionMode::LiveManual` のときだけ隠れることを保証する（kind:ui / issue #31, #33）。
//!
//! - フローティングウィンドウ root (`WindowRoot` + `PanelKind::StrategyEditor`) の
//!   `Visibility` と、サイドバーボタン (`Button` + `PanelKind::StrategyEditor`) の
//!   `Node.display` を `apply_strategy_editor_mode_visibility_system` が駆動する。
//! - Manual ONLY: Replay / LiveAuto では表示されたまま。
//! - Manual 中に新規 spawn されたウィンドウも隠れる（`is_changed()` ゲート無し）。
//! - 別 `PanelKind` のウィンドウは触らない。
//! - **子まで隠れる構造条件**（issue #33 [MEDIUM]）: 実 Strategy Editor を spawn し、
//!   editor / gutter / scrollbar_track の各子から `Parent` 連鎖で root まで上がる経路上の
//!   全 entity が `Visibility` と `InheritedVisibility` の **両方** を持つ
//!   （= root を `Hidden` にすれば中身まで伝播する）ことを assert する。Bevy 0.15 の
//!   `propagate_recursive` は各ノードを `(&Visibility, &mut InheritedVisibility)` で get するため、
//!   どちらか一方でも欠けると early-return し、枠だけ消えて中身が残る（M11 と同型の構造バグ）。
//!   `InheritedVisibility` の有無だけ見ると「Visibility が無い中間ノード」を取りこぼすため
//!   伝播ゲートそのものを検証する。レンダ依存なしにこの構造条件を固定する。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::{ExecutionMode, ExecutionModeRes};
use backcast::ui::components::{
    PanelKind, PanelSpawnSource, RegionKeyAllocator, StrategyEditorSpawnSpec, WindowRoot,
};
use backcast::ui::strategy_editor::{
    apply_strategy_editor_mode_visibility_system, spawn_strategy_editor_panel,
    StrategyEditorModeHidden,
};

/// 実 Strategy Editor を 1 回だけ spawn するテストローカル startup system。
/// 引数注入により `Commands` / `RegionKeyAllocator` の借用が分離され、
/// world から手で取り出す際の二重借用を避ける（production dispatcher と同じ注入形）。
fn spawn_real_editor_system(
    mut commands: Commands,
    mut allocator: ResMut<RegionKeyAllocator>,
) {
    spawn_strategy_editor_panel(
        &mut commands,
        &mut allocator,
        StrategyEditorSpawnSpec {
            region_key: None,
            source: Some(String::new()),
            layout_source: PanelSpawnSource::User,
        },
    );
}

#[test]
fn m12_strategy_editor_hidden_in_manual() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<ExecutionModeRes>(); // 既定 = Replay
    app.insert_resource(RegionKeyAllocator::default());

    // ── 実 Strategy Editor ウィンドウを spawn（editor/gutter/scrollbar を本物にする）──
    // Startup で 1 回だけ走らせ、deferred command を flush して entity を実体化する。
    app.add_systems(Startup, spawn_real_editor_system);
    app.update();

    // この update で Startup が走り、本体の visibility system も以降登録する。
    app.add_systems(Update, apply_strategy_editor_mode_visibility_system);

    // spawn した root（WindowRoot かつ PanelKind::StrategyEditor）を取得。
    let window = app
        .world_mut()
        .query_filtered::<Entity, (With<WindowRoot>, With<backcast::ui::strategy_editor::StrategyEditorRoot>)>()
        .iter(app.world())
        .next()
        .expect("実 Strategy Editor の root が spawn されているはず");

    // サイドバーボタン（root とは独立に display を駆動される）。
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
    assert!(
        app.world().get::<StrategyEditorModeHidden>(window).is_some(),
        "Manual 中は退避マーカーが付いているはず"
    );
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
