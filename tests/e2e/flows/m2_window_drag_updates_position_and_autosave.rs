//! M2 window_drag_updates_position_and_autosave — floating window の title bar を
//! ドラッグすると位置が更新され、DragEnd で `AutoSaveState.dirty = true` になることを
//! 保証する（kind:ui/integration）。
//!
//! `spawn_floating_window` で本番 window を生成し、title bar の `Pointer<Drag>` /
//! `Pointer<DragStart>` / `Pointer<DragEnd>` observer を `world.trigger_targets` で
//! 直接発火する。OS イベントループ・GPU・picking backend は不要。
//!
//! DragEnd observer は `chart_instrument.is_some()` のときだけ early-return するため、
//! Chart 以外の window ならば必ず auto_save.dirty が true になる。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy::picking::pointer::{Location, PointerId};
use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::render::camera::NormalizedRenderTarget;
use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PendingStrategyFragments, RegionKeyAllocator,
    StrategyBuffer, WindowManager,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use backcast::ui::layout_persistence::AutoSaveState;

use crate::ui_dump::dump_panels;

/// ダミーの `Location` を作るヘルパー（picking backend 不要な headless 用）。
/// observer 内では pointer_location を参照しないため、image target で代替する。
fn dummy_location() -> Location {
    Location {
        target: NormalizedRenderTarget::Image(Handle::<bevy::image::Image>::default()),
        position: Vec2::ZERO,
    }
}

#[test]
fn m2_window_drag_updates_position_and_autosave() {
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

    // Camera2d を 1 体置く（title bar drag observer が get_single() するため）。
    app.world_mut()
        .spawn((Camera2d, Transform::default(), OrthographicProjection::default_2d()));

    // 本番 spawn_floating_window で BuyingPower window を生成する。
    let (root, _content, title_bar) = {
        let mut commands = app.world_mut().commands();
        let result = spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "BUYING POWER".to_string(),
                size: Vec2::new(270.0, 130.0),
                position: Vec2::new(100.0, 200.0),
                accent: Color::srgba(0.0, 0.8, 1.0, 0.4),
            },
        );
        result
    };
    // Commands を flush して observers を確定させる。
    app.world_mut().flush();

    // PanelKind を root に付ける（despawn 判定などで必要）。
    app.world_mut().entity_mut(root).insert(PanelKind::BuyingPower);
    app.update();

    // ── Phase A: 元の位置を記録する ──
    let pos_before = app
        .world()
        .get::<Transform>(root)
        .map(|t| t.translation.truncate())
        .expect("root は Transform を持つはず");

    // ── Phase B: DragStart → Drag → DragEnd を title bar に発火する ──
    let loc = dummy_location();

    // DragStart: ActiveDrag.starts に元位置を登録する。
    app.world_mut().trigger_targets(
        Pointer::<DragStart>::new(
            title_bar,
            PointerId::Mouse,
            loc.clone(),
            DragStart {
                button: bevy::picking::pointer::PointerButton::Primary,
                hit: bevy::picking::backend::HitData::new(Entity::from_raw(0), 0.0, None, None),
            },
        ),
        title_bar,
    );

    // Drag: title bar の Pointer<Drag> observer が親(root)の Transform を動かす。
    // delta は world-space で (+50, +30) 移動。
    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            title_bar,
            PointerId::Mouse,
            loc.clone(),
            Drag {
                button: bevy::picking::pointer::PointerButton::Primary,
                distance: Vec2::new(50.0, 30.0),
                delta: Vec2::new(50.0, -30.0), // screen Y を反転して world Y へ
            },
        ),
        title_bar,
    );
    app.update();

    // 位置が変化していること。
    let pos_after = app
        .world()
        .get::<Transform>(root)
        .map(|t| t.translation.truncate())
        .expect("root は Transform を持つはず");

    assert_ne!(
        pos_before, pos_after,
        "Drag 後に position が変化するはず (before={pos_before:?}, after={pos_after:?})"
    );

    // ── Phase C: DragEnd で auto_save.dirty が true になること ──
    app.world_mut().trigger_targets(
        Pointer::<DragEnd>::new(
            title_bar,
            PointerId::Mouse,
            loc.clone(),
            DragEnd {
                button: bevy::picking::pointer::PointerButton::Primary,
                distance: Vec2::new(50.0, 30.0),
            },
        ),
        title_bar,
    );
    app.update();

    let dirty = app.world().resource::<AutoSaveState>().dirty;
    assert!(dirty, "DragEnd 後に AutoSaveState.dirty = true のはず");

    // ── Phase D: dump_panels で位置も確認する ──
    let panels = dump_panels(app.world_mut());
    let bp = panels
        .iter()
        .find(|p| p.kind == "Buying Power")
        .expect("Buying Power panel が dump_panels に現れるはず");
    // 元の position (100, 200) から離れていること。
    assert_ne!(
        bp.position,
        Vec2::new(100.0, 200.0),
        "dump_panels の position もドラッグ後の値のはず (got {:?})",
        bp.position
    );
}
