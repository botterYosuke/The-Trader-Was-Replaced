//! K18 live_chart_resize_reflow — Live 複合パネル（chart + ladder）の右端 resize handle を
//! Drag すると chart Sprite・price_gutter・ladder pane が新しい描画幅に追従することを保証する
//! (kind:ui)。
//!
//! - chart draw_w = root_w - PRICE_GUTTER_WIDTH - LADDER_WIDTH
//! - ladder pane Transform.x = root_w / 2.0 - LADDER_WIDTH / 2.0
//!
//! Slice 2 実装前: chart は PRICE_GUTTER_WIDTH しか引かず ladder 分の draw_w が広すぎる、
//!   かつ ladder.x が静止する → 両 assert が RED で fail する。
//! Slice 2 実装後: chart / ladder が新しい描画幅に更新される → assert が GREEN で pass する。

use bevy::picking::events::{Drag, Pointer};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::render::camera::NormalizedRenderTarget;
use bevy::transform::TransformPlugin;
use backcast::ui::chart_axes::{PriceGutter, PriceGutterRef, TimeGutterRef};
use backcast::ui::chart_viewstate::{
    LADDER_WIDTH, LIVE_COMBINED_PANEL_SIZE, PRICE_GUTTER_WIDTH, TIME_GUTTER_HEIGHT,
};
use backcast::ui::chart_ladder_pane::LadderPane;
use backcast::ui::chart_viewstate::ChartViewState;
use backcast::ui::components::{
    ChartInstrument, InstrumentRegistry, PanelKind, RegionKeyAllocator,
};
use backcast::ui::floating_window::{
    FloatingWindowChildren, FloatingWindowSpec, TITLE_BAR_HEIGHT, spawn_floating_window,
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
fn k18_live_chart_resize_reflow() {
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

    let initial_size = LIVE_COMBINED_PANEL_SIZE; // Vec2(480.0, 244.0)

    let (root, content_area, _title_bar) = {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "CHART LIVE — TEST".to_string(),
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

    // Live 複合の chart draw_w = total - PRICE_GUTTER - LADDER
    let initial_draw_w = initial_size.x - PRICE_GUTTER_WIDTH - LADDER_WIDTH;
    let initial_draw_h = initial_size.y - TITLE_BAR_HEIGHT - TIME_GUTTER_HEIGHT;

    // chart Sprite entity を content_area の子として spawn する
    let chart = {
        let mut commands = app.world_mut().commands();
        let e = commands
            .spawn((
                Sprite {
                    custom_size: Some(Vec2::new(initial_draw_w, initial_draw_h)),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.001),
                    ..default()
                },
                Transform::from_xyz(
                    -(PRICE_GUTTER_WIDTH + LADDER_WIDTH) / 2.0,
                    TIME_GUTTER_HEIGHT / 2.0,
                    0.1,
                ),
                ChartViewState {
                    bounds: Vec2::new(initial_draw_w, initial_draw_h),
                    ..default()
                },
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
                    initial_draw_w / 2.0 + PRICE_GUTTER_WIDTH / 2.0,
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
                Transform::from_xyz(
                    0.0,
                    -initial_draw_h / 2.0 - TIME_GUTTER_HEIGHT / 2.0,
                    0.1,
                ),
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

    // ladder pane を content_area の子として spawn する
    let ladder = {
        let mut commands = app.world_mut().commands();
        let ladder_h = initial_size.y - TITLE_BAR_HEIGHT;
        let ladder_x = initial_size.x / 2.0 - LADDER_WIDTH / 2.0;
        let e = commands
            .spawn((
                LadderPane {
                    chart_root: root,
                    last_depth_signature: 0,
                },
                Sprite {
                    custom_size: Some(Vec2::new(LADDER_WIDTH, ladder_h)),
                    color: Color::srgba(0.08, 0.08, 0.08, 0.95),
                    ..default()
                },
                Transform::from_xyz(ladder_x, 0.0, 0.2),
            ))
            .id();
        commands.entity(content_area).add_child(e);
        e
    };
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

    // ── assert A: root 幅が増加している ──
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

    // ── assert B: chart Sprite が Live 描画幅 (ladder 分を引いた幅) に追従する (RED: Slice 2 前は fail) ──
    let expected_draw_w = new_root_w - PRICE_GUTTER_WIDTH - LADDER_WIDTH;
    let chart_size = app
        .world()
        .get::<Sprite>(chart)
        .and_then(|s| s.custom_size)
        .expect("chart は Sprite を持つはず");
    assert_eq!(
        chart_size.x,
        expected_draw_w,
        "RED: chart.x={} は expected_draw_w={} に追従するはず (PRICE_GUTTER+LADDER 両方引く)",
        chart_size.x,
        expected_draw_w,
    );

    // ── assert C: price_gutter Transform.x が新しい描画幅に追従する (RED: Slice 2 前は fail) ──
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
        "RED: gutter.x={} は expected_gutter_x={} に追従するはず",
        gutter_x,
        expected_gutter_x,
    );

    // ── assert D: ladder pane Transform.x が新しい右端に追従する (RED: Slice 2 前は fail) ──
    let expected_ladder_x = new_root_w / 2.0 - LADDER_WIDTH / 2.0;
    let ladder_x = app
        .world()
        .get::<Transform>(ladder)
        .expect("ladder は Transform を持つはず")
        .translation
        .x;
    assert_eq!(
        ladder_x,
        expected_ladder_x,
        "RED: ladder.x={} は expected_ladder_x={} に追従するはず (右端フラッシュ)",
        ladder_x,
        expected_ladder_x,
    );

    // ── assert E: chart 右端 + gutter 右端 + ladder 左端 — 重なりが無い (幾何的無重複) ──
    // chart 中心 x = -(PRICE_GUTTER_WIDTH + LADDER_WIDTH) / 2 (不変)
    // chart 右端 (chart-local) = expected_draw_w / 2
    // gutter 左端 (chart-local) = expected_gutter_x - PRICE_GUTTER_WIDTH / 2 = expected_draw_w / 2
    // よって chart 右端 == gutter 左端 → 隙間なし重なりなし
    let chart_right = chart_size.x / 2.0;
    let gutter_left = gutter_x - PRICE_GUTTER_WIDTH / 2.0;
    assert!(
        (chart_right - gutter_left).abs() < 0.01,
        "chart 右端 {} と gutter 左端 {} が隣接するはず (重なり/隙間なし)",
        chart_right,
        gutter_left,
    );
}
