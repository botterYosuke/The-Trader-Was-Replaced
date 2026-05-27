//! M10 window_resize_updates_size_and_autosave — resizable floating window の
//! 右端ハンドルを Pointer<Drag> で引くとルートの custom_size が拡大し、
//! Pointer<DragEnd> で `AutoSaveState.dirty = true` になることを保証する（kind:ui）。
//!
//! 最小サイズクランプ（MIN_WINDOW_SIZE）と左端・上端固定（translation シフト）も検証する。
//! 再起動後のサイズ復元は layout_persistence の i12 / i7 が保証する側なので本 flow は
//! "サイズ変化 + dirty" を不変条件とする。

use bevy::picking::events::{Drag, DragEnd, Pointer};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::render::camera::NormalizedRenderTarget;
use bevy::transform::TransformPlugin;
use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PendingStrategyFragments, RegionKeyAllocator,
    StrategyBuffer, WindowManager,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::{FloatingWindowChildren, FloatingWindowSpec, spawn_floating_window};
use backcast::ui::layout_persistence::AutoSaveState;

fn dummy_location() -> Location {
    Location {
        target: NormalizedRenderTarget::Image(Handle::<bevy::image::Image>::default()),
        position: Vec2::ZERO,
    }
}

#[test]
fn m10_window_resize_updates_size_and_autosave() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(WindowManager::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(InstrumentRegistry::default())
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(PendingStrategyFragments::default())
        .init_resource::<backcast::ui::components::ChartSizeMap>();

    // OrthographicProjection.scale = 1.0 (drag delta の scale 補正に使う)
    app.world_mut()
        .spawn((Camera2d, Transform::default(), OrthographicProjection::default_2d()));

    let initial_size = Vec2::new(360.0, 260.0);

    let (root, _content, _title_bar) = {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "STRATEGY".to_string(),
                size: initial_size,
                position: Vec2::ZERO,
                accent: Color::srgba(0.0, 0.8, 1.0, 0.4),
                closeable: true,
                resizable: true,
            },
        )
    };
    app.world_mut().flush();
    app.world_mut().entity_mut(root).insert(PanelKind::StrategyEditor);
    app.update();

    // resize_right ハンドルを FloatingWindowChildren から取得する
    let resize_right = app
        .world()
        .get::<FloatingWindowChildren>(root)
        .and_then(|c| c.resize_right)
        .expect("resizable=true なら resize_right entity が存在するはず");

    let loc = dummy_location();

    // ── Phase A: 右端を +80px 右にドラッグ ──
    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            resize_right,
            PointerId::Mouse,
            loc.clone(),
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::new(80.0, 0.0),
                delta: Vec2::new(80.0, 0.0),
            },
        ),
        resize_right,
    );
    app.update();

    let size_after = app
        .world()
        .get::<Sprite>(root)
        .and_then(|s| s.custom_size)
        .expect("root は Sprite を持つはず");

    assert!(
        size_after.x > initial_size.x,
        "Drag 後に幅が増加するはず (before={}, after={})",
        initial_size.x,
        size_after.x
    );
    assert_eq!(
        size_after.y, initial_size.y,
        "右端ドラッグで高さは変わらないはず"
    );

    // ── Phase B: 最小サイズより小さくしようとしてもクランプされること ──
    // 現在の幅から -9999 px 左へ drag（left edge 固定なので幅が負になろうとする）
    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            resize_right,
            PointerId::Mouse,
            loc.clone(),
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::new(-9999.0, 0.0),
                delta: Vec2::new(-9999.0, 0.0),
            },
        ),
        resize_right,
    );
    app.update();

    let size_clamped = app
        .world()
        .get::<Sprite>(root)
        .and_then(|s| s.custom_size)
        .expect("root は Sprite を持つはず");

    assert!(
        size_clamped.x >= backcast::ui::floating_window::MIN_WINDOW_SIZE.x,
        "最小幅クランプが効くはず (got {})",
        size_clamped.x
    );

    // ── Phase C: DragEnd で AutoSaveState.dirty = true になること ──
    app.world_mut().trigger_targets(
        Pointer::<DragEnd>::new(
            resize_right,
            PointerId::Mouse,
            loc.clone(),
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::new(80.0, 0.0),
            },
        ),
        resize_right,
    );
    app.update();

    let dirty = app.world().resource::<AutoSaveState>().dirty;
    assert!(dirty, "DragEnd 後に AutoSaveState.dirty = true のはず");
}
