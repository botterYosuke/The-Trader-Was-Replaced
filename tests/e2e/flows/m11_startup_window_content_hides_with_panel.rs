//! M11 startup_window_content_hides_with_panel — Manual/Auto モードで Startup ウィンドウを
//! 隠したとき、枠だけでなく **中身（Start/End/Granularity/Initial cash）も隠れる**ことを保証する
//! （kind:ui）。[M8] は root の `Visibility` 切替だけを見ており、中身が残るバグ（root 枠は
//! 消えるが Start/End… が表示されたまま）を検出できなかった。その回帰テスト。
//!
//! Bevy 0.15 の `visibility_propagate_system` は root→子 を辿る際、途中の entity が
//! `(&Visibility, &mut InheritedVisibility)` を欠くと `propagate_recursive` が early-return し、
//! そこで連鎖が切れる。`spawn_floating_window` の `content_area` が `Visibility` を持たないと
//! まさにこの連鎖断ちが起き、root を `Hidden` にしても中身が隠れない。
//!
//! ここでは描画プラグインを足さず、**「content 子から root までの祖先が全員
//! `InheritedVisibility` を持つ（＝伝播が中身まで届く構造である）」** という不変条件を assert する。
//! これは伝播がコンテンツに届くための必要十分な構造条件で、レンダ依存なしに root-cause を固定できる。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::trading::ExecutionModeRes;
use backcast::ui::components::{ScenarioStartupPanelRoot, ScenarioStartupStartFieldHost, WindowRoot};
use backcast::ui::scenario_startup_panel::spawn_scenario_startup_window;

#[test]
fn m11_startup_window_content_hides_with_panel() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.init_resource::<ExecutionModeRes>();

    {
        let mut commands = app.world_mut().commands();
        spawn_scenario_startup_window(&mut commands);
    }
    app.world_mut().flush();

    let world = app.world_mut();

    // root（= ScenarioStartupPanelRoot かつ WindowRoot）を取得。
    let root = world
        .query_filtered::<Entity, (With<ScenarioStartupPanelRoot>, With<WindowRoot>)>()
        .iter(world)
        .next()
        .expect("Startup ウィンドウ root が存在するはず");

    // root 自身は Sprite 由来で Visibility/InheritedVisibility を持つ。
    assert!(
        world.get::<InheritedVisibility>(root).is_some(),
        "root は InheritedVisibility を持つはず（可視性伝播の起点）"
    );

    // content 子（Start フィールド host）を起点に、Parent を辿って root まで上がり、
    // 経路上の **全 entity** が InheritedVisibility を持つ（伝播の連鎖が切れていない）ことを確認。
    let start_host = world
        .query_filtered::<Entity, With<ScenarioStartupStartFieldHost>>()
        .iter(world)
        .next()
        .expect("Start フィールド host が存在するはず");

    let mut cursor = start_host;
    let mut reached_root = false;
    // 深さガード（壊れた階層での無限ループ防止）。
    for _ in 0..32 {
        assert!(
            world.get::<InheritedVisibility>(cursor).is_some(),
            "content から root への経路上の entity {cursor:?} が InheritedVisibility を欠く \
             → ここで可視性伝播が途切れ、root を Hidden にしても中身が隠れない（本バグ）"
        );
        if cursor == root {
            reached_root = true;
            break;
        }
        match world.get::<Parent>(cursor) {
            Some(parent) => cursor = parent.get(),
            None => break,
        }
    }
    assert!(
        reached_root,
        "Start フィールド host は Parent 連鎖で root へ到達するはず（content_area 経由）"
    );
}
