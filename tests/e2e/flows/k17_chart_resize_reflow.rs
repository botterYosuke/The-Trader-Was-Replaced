//! K17 chart_resize_reflow — Chart panel の右端 resize handle を Drag すると
//! chart Sprite と price_gutter Transform が新しい描画幅に追従することを保証する (kind:ui)。
//!
//! Slice 1 実装前: chart / gutter は静的 CHART_DRAW_SIZE のまま → assert が RED で fail する。
//! Slice 1 実装後: chart / gutter が新しい描画幅に更新される → assert が GREEN で pass する。
//!
//! RED＝回帰ガード・fix は chart_resize Slice 1 後に green

use bevy::picking::events::{Drag, Pointer};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::render::camera::NormalizedRenderTarget;
use bevy::transform::TransformPlugin;
use backcast::ui::chart_axes::PriceGutter;
use backcast::ui::chart_axes::PriceGutterRef;
use backcast::ui::chart_axes::TimeGutterRef;
use backcast::ui::chart_viewstate::ChartViewState;
use backcast::ui::chart_viewstate::{
    CHART_DRAW_SIZE, CHART_PANEL_SIZE, PRICE_GUTTER_WIDTH, TIME_GUTTER_HEIGHT,
};
use backcast::ui::components::{
    ChartInstrument, InstrumentRegistry, PanelKind, RegionKeyAllocator,
};
use backcast::ui::floating_window::{
    FloatingWindowChildren, FloatingWindowSpec, spawn_floating_window,
};
use backcast::ui::window::{ChartLayoutChildren, chart_content_layout_system};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::layout_persistence::AutoSaveState;

fn dummy_location() -> Location {
    Location {
        target: NormalizedRenderTarget::Image(Handle::<bevy::image::Image>::default()),
        position: Vec2::ZERO,
    }
}

#[test]
fn k17_chart_resize_reflow() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.add_systems(Update, chart_content_layout_system);

    app.insert_resource(InstrumentRegistry::default())
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default());

    // OrthographicProjection.scale = 1.0 (drag delta の scale 補正に使う)
    app.world_mut()
        .spawn((Camera2d, Transform::default(), OrthographicProjection::default_2d()));

    let initial_size = CHART_PANEL_SIZE; // Vec2(360.0, 244.0)

    let (root, content_area, _title_bar) = {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "CHART — TEST".to_string(),
                size: initial_size,
                position: Vec2::ZERO,
                accent: Color::srgba(0.0, 0.8, 1.0, 0.4),
                closeable: true,
                resizable: true,
            },
        )
    };
    app.world_mut().flush();

    // price_text（ChartLayoutChildren に必要なダミー）
    let price_text = app.world_mut().spawn(Transform::default()).id();
    app.world_mut().entity_mut(content_area).add_child(price_text);
    app.world_mut().flush();

    app.world_mut().entity_mut(root).insert(PanelKind::Chart);
    app.world_mut().entity_mut(root).insert(ChartInstrument {
        instrument_id: "TEST".to_string(),
    });

    // chart Sprite entity を content_area の子として spawn する
    let chart = {
        let mut commands = app.world_mut().commands();
        let e = commands
            .spawn((
                Sprite {
                    custom_size: Some(CHART_DRAW_SIZE),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.001),
                    ..default()
                },
                Transform::from_xyz(-PRICE_GUTTER_WIDTH / 2.0, TIME_GUTTER_HEIGHT / 2.0, 0.1),
                ChartViewState { bounds: CHART_DRAW_SIZE, ..default() },
            ))
            .id();
        commands.entity(content_area).add_child(e);
        e
    };
    app.world_mut().flush();

    // price_gutter を chart の子として spawn する
    let price_gutter = {
        let mut commands = app.world_mut().commands();
        let e = commands
            .spawn((
                PriceGutter,
                Transform::from_xyz(
                    CHART_DRAW_SIZE.x / 2.0 + PRICE_GUTTER_WIDTH / 2.0,
                    0.0,
                    0.1,
                ),
                Visibility::default(),
            ))
            .id();
        commands.entity(chart).add_child(e);
        e
    };
    let time_gutter = {
        let mut commands = app.world_mut().commands();
        let e = commands
            .spawn((
                Transform::from_xyz(0.0, -CHART_DRAW_SIZE.y / 2.0 - TIME_GUTTER_HEIGHT / 2.0, 0.1),
                Visibility::default(),
            ))
            .id();
        commands.entity(chart).add_child(e);
        e
    };
    app.world_mut().entity_mut(root).insert(ChartLayoutChildren { chart, price_text });
    app.world_mut().commands().entity(chart).insert((
        PriceGutterRef(price_gutter),
        TimeGutterRef(time_gutter),
    ));
    app.world_mut().flush();
    app.update();

    // resize_right ハンドルを FloatingWindowChildren から取得する
    let resize_right = app
        .world()
        .get::<FloatingWindowChildren>(root)
        .and_then(|c| c.resize_right)
        .expect("resizable=true なら resize_right entity が存在するはず");

    // ── Drag +80px 右端 ──
    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            resize_right,
            PointerId::Mouse,
            dummy_location(),
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::new(80.0, 0.0),
                delta: Vec2::new(80.0, 0.0),
            },
        ),
        resize_right,
    );
    app.update();

    // ── assert A: root 幅が増加している（Slice 1 実装前から PASS する基礎確認）──
    let new_root_w = app
        .world()
        .get::<Sprite>(root)
        .and_then(|s| s.custom_size)
        .expect("root は Sprite を持つはず")
        .x;
    assert!(
        new_root_w > initial_size.x,
        "root 幅が増加するはず (before={}, after={})",
        initial_size.x,
        new_root_w,
    );

    // ── assert B: chart Sprite が新しい描画幅に追従する（RED: Slice 1 実装前は fail）──
    let expected_draw_w = new_root_w - PRICE_GUTTER_WIDTH;
    let chart_size = app
        .world()
        .get::<Sprite>(chart)
        .and_then(|s| s.custom_size)
        .expect("chart は Sprite を持つはず");
    assert_eq!(
        chart_size.x,
        expected_draw_w,
        "RED: chart.x={} は expected_draw_w={} に追従するはず (Slice 1 実装前は fail)",
        chart_size.x,
        expected_draw_w,
    );

    // ── assert C: price_gutter Transform.x が新しい描画幅に追従する（RED: Slice 1 実装前は fail）──
    let expected_gutter_x = expected_draw_w / 2.0 + PRICE_GUTTER_WIDTH / 2.0;
    let gutter_x = app
        .world()
        .get::<Transform>(price_gutter)
        .expect("price_gutter は Transform を持つはず")
        .translation
        .x;
    assert_eq!(
        gutter_x,
        expected_gutter_x,
        "RED: gutter.x={} は expected_gutter_x={} に追従するはず (Slice 1 実装前は fail)",
        gutter_x,
        expected_gutter_x,
    );
}
