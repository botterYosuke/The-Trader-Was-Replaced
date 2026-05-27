//! M4 window_focus_brings_to_front — floating window の本体（root sprite）に
//! `Pointer<Down>` を発火すると `WindowManager.max_z += 2` されて
//! その window の z が他の window より高くなることを保証する（kind:ui）。
//!
//! `spawn_floating_window` の WindowRoot に貼られた observer:
//! ```
//! wm.max_z += 2.0;
//! transform.translation.z = 10.0 + wm.max_z;
//! ```
//! を `world.trigger_targets` で直接発火する。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy::picking::pointer::{Location, PointerId, PointerButton};
use bevy::picking::events::{Press, Pointer};
use bevy::camera::NormalizedRenderTarget;

use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PendingStrategyFragments, RegionKeyAllocator,
    StrategyBuffer, WindowManager,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use backcast::ui::layout_persistence::AutoSaveState;

/// ダミーの `Location` を作るヘルパー（picking backend 不要な headless 用）。
/// observer 内では pointer_location を参照しないため、image target で代替する。
fn dummy_location() -> Location {
    Location {
        target: NormalizedRenderTarget::Image(Handle::<bevy::image::Image>::default().into()),
        position: Vec2::ZERO,
    }
}

#[test]
fn m4_window_focus_brings_to_front() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(WindowManager::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(InstrumentRegistry::default())
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(PendingStrategyFragments::default());

    // Window A（最初に spawn → z=10.0）
    let (root_a, _, _) = {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "Window A".to_string(),
                size: Vec2::new(200.0, 150.0),
                position: Vec2::new(-100.0, 0.0),
                accent: Color::srgba(0.0, 0.5, 1.0, 0.4),
                closeable: true,
                resizable: false,
            },
        )
    };
    app.world_mut().flush();
    app.world_mut().entity_mut(root_a).insert(PanelKind::BuyingPower);

    // Window B（2 番目に spawn → z=10.0 だが同一フレームなので初期値は同じ）
    let (root_b, _, _) = {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "Window B".to_string(),
                size: Vec2::new(200.0, 150.0),
                position: Vec2::new(100.0, 0.0),
                accent: Color::srgba(1.0, 0.5, 0.0, 0.4),
                closeable: true,
                resizable: false,
            },
        )
    };
    app.world_mut().flush();
    app.world_mut().entity_mut(root_b).insert(PanelKind::RunResult);

    app.update();

    // Window A の初期 z を記録する。
    let z_a_before = app
        .world()
        .get::<Transform>(root_a)
        .map(|t| t.translation.z)
        .expect("root_a は Transform を持つはず");

    // ── Window A に Pointer<Press> を発火して前面化 ──
    app.world_mut().entity_mut(root_a).trigger(|entity| {
        Pointer::<Press>::new(
            PointerId::Mouse,
            dummy_location(),
            Press {
                button: PointerButton::Primary,
                hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    });
    app.update();

    let z_a_after = app
        .world()
        .get::<Transform>(root_a)
        .map(|t| t.translation.z)
        .expect("root_a は Transform を持つはず");
    let z_b = app
        .world()
        .get::<Transform>(root_b)
        .map(|t| t.translation.z)
        .expect("root_b は Transform を持つはず");

    // クリック後、Window A の z が上がっていること。
    assert!(
        z_a_after > z_a_before,
        "Pointer<Down> 後に z が上がるはず (before={z_a_before}, after={z_a_after})"
    );

    // Window A の z が Window B より高くなっていること（前面化）。
    assert!(
        z_a_after > z_b,
        "前面化後は root_a.z > root_b.z のはず (a={z_a_after}, b={z_b})"
    );

    // WindowManager.max_z が 2 増えていること。
    let wm = app.world().resource::<WindowManager>();
    assert!(
        wm.max_z >= 2.0,
        "WindowManager.max_z >= 2 のはず (got {})",
        wm.max_z
    );
}
