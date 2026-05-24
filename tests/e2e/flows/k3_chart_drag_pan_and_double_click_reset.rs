//! K3 chart_drag_pan_and_double_click_reset — 左ドラッグで chart 内 pan、右/中ドラッグで canvas pan、
//! ダブルクリックで pan/zoom reset と autoscale 再有効化が起きることを保証する（kind:ui）。
//!
//! ## 設計メモ
//! `install_chart_drag_observer` / `install_chart_autoscale_reset_observer` はどちらも
//! `Added<ChartViewState>` を gate に chart entity へ `Pointer<Drag>` / `Pointer<Click>` の
//! observer を **entity に直接貼る** (Bevy 0.15 の entity-scoped observer)。
//!
//! headless では `app.world_mut().trigger_targets(event, entity)` で observer を発火できる。
//! `Pointer<Drag>` の構築に必要な `Location { target: NormalizedRenderTarget, .. }` は
//! `NormalizedRenderTarget::Image(Handle::default())` で dummy 生成する（viewport 計算には不使用）。
//!
//! `ChartClickState` のフィールドは非公開のため、ダブルクリックは 2 連続 click イベントで
//! 自然に発生させる（Time<()>.elapsed_secs() が 0.0 に近いため 2 click が 0.4s 閾値内に入る）。
//!
//! テストは production observer を通る経路で：
//! 1. 左ドラッグ → `translation` が delta 分変化し `auto_scale = false` になること。
//! 2. 右ドラッグ → chart の `translation` は **変化しない**（左ボタンのみ chart pan）。
//! 3. 2 連続クリック → double-click 判定で `reset_view()` が呼ばれ `translation == Vec2::ZERO` / `auto_scale = true`。
//! 4. ドラッグ後の click → double-click 列に入らず reset しない（pan 2 連発誤検出ガード）。

use std::time::Duration;

use bevy::picking::backend::HitData;
use bevy::picking::events::{Click, Drag, Pointer};
use bevy::picking::pointer::{Location, PointerId, PointerButton};
use bevy::prelude::*;
use bevy::render::camera::NormalizedRenderTarget;

use backcast::ui::chart_interaction::{
    install_chart_autoscale_reset_observer, install_chart_drag_observer, ChartClickState,
};
use backcast::ui::chart_viewstate::{ChartViewState, DEFAULT_CELL_WIDTH};
use backcast::ui::components::ChartInstrument;

/// headless テスト用の dummy `Location`。viewport 計算には使われない。
fn dummy_location() -> Location {
    Location {
        target: NormalizedRenderTarget::Image(bevy::render::camera::ImageRenderTarget { handle: Handle::default(), scale_factor: bevy::math::FloatOrd(1.0) }),
        position: Vec2::ZERO,
    }
}

/// headless テスト用の dummy `HitData`。
fn dummy_hit() -> HitData {
    HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
}

#[test]
fn k3_chart_drag_pan_and_double_click_reset() {
    let mut app = App::new();
    app.add_plugins(bevy::app::TaskPoolPlugin::default());
    app.add_plugins(bevy::app::ScheduleRunnerPlugin::default());
    app.add_plugins(bevy::time::TimePlugin);

    app.init_resource::<ChartClickState>();
    app.insert_resource(ButtonInput::<KeyCode>::default());

    // observer インストーラーを登録。Added<ChartViewState> + With<Sprite> で起動。
    app.add_systems(
        Update,
        (
            install_chart_drag_observer,
            install_chart_autoscale_reset_observer,
        ),
    );

    // chart entity を spawn する。次フレームで observer がインストールされる。
    let chart = app
        .world_mut()
        .spawn((
            ChartViewState::default(),
            ChartInstrument {
                instrument_id: "1301.TSE".to_string(),
            },
            Sprite::default(),          // install_chart_drag_observer の With<Sprite> gate に必要
            Transform::default(),
            GlobalTransform::default(),
        ))
        .id();

    // observer install フレーム。
    app.update();

    // ── Case 1: 左ドラッグ → translation 変化 + auto_scale = false ──
    // delta = (10, 5): translation.x += 10, translation.y -= 5 (Bevy Y は上が正)。
    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::new(10.0, 5.0),
                delta: Vec2::new(10.0, 5.0),
            },
        ),
        chart,
    );
    app.update();

    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        // scale=1.0 (カメラなし → unwrap_or(1.0)) なので delta がそのまま加算される。
        assert!(
            (state.translation.x - 10.0).abs() < 1e-3,
            "左ドラッグで translation.x が delta.x 分増えるはず: {:?}",
            state.translation
        );
        assert!(
            (state.translation.y - (-5.0)).abs() < 1e-3,
            "左ドラッグで translation.y が -delta.y になるはず (Bevy Y上正): {:?}",
            state.translation
        );
        assert!(
            !state.auto_scale,
            "左ドラッグで auto_scale が false になるはず"
        );
    }

    // ── Case 2: 右ドラッグ → chart pan しない ──
    // 右ボタンは camera.rs の canvas pan なので chart の translation は動かない。
    let before_translation = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .translation;

    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Drag {
                button: PointerButton::Secondary, // 右ボタン: 早期 return
                distance: Vec2::new(20.0, 20.0),
                delta: Vec2::new(20.0, 20.0),
            },
        ),
        chart,
    );
    app.update();

    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert_eq!(
            state.translation, before_translation,
            "右ドラッグでは chart translation は変化しない（canvas pan に委譲）"
        );
    }

    // ── Case 3: 2 連続クリック → double-click → reset_view() ──
    // pan/zoom 済み状態を準備する。
    {
        let mut s = app
            .world_mut()
            .get_mut::<ChartViewState>(chart)
            .unwrap();
        s.translation = Vec2::new(50.0, -30.0);
        s.cell_width = 20.0;
        s.auto_scale = false;
    }

    // Case 1 の左ドラッグで `ChartClickState.dragged` に印が残っている (production では
    // ドラッグ後の pointer-up が drag 由来 Click を 1 発生み、その Click が印を消費する)。
    // この drag 由来 click を 1 回流して dragged 印を消費し、以降の click を genuine 扱いにする。
    // (ChartClickState のフィールドは非公開なのでテストから直接 clear できない。)
    app.world_mut().trigger_targets(
        Pointer::<Click>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Click {
                button: PointerButton::Primary,
                hit: dummy_hit(),
                duration: Duration::from_millis(50),
            },
        ),
        chart,
    );
    app.update();

    // 1 click 目: dragged フラグ無し → last_click に時刻が入る。
    // headless では Time elapsed_secs が 0.0 に近い（フレームは極小時間）ので
    // 2 click の時刻差 ≤ DOUBLE_CLICK_SECS (0.4s) が自然に成立する。
    app.world_mut().trigger_targets(
        Pointer::<Click>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Click {
                button: PointerButton::Primary,
                hit: dummy_hit(),
                duration: Duration::from_millis(80),
            },
        ),
        chart,
    );
    app.update();

    // 1 click 後: reset はまだされていない（double-click 判定未成立）。
    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert!(
            !state.auto_scale,
            "1 click 目は reset しない（double-click 未成立）: auto_scale={}",
            state.auto_scale
        );
    }

    // 2 click 目: 前の click から delta_t ≈ 0 なので double-click 成立 → reset_view()。
    app.world_mut().trigger_targets(
        Pointer::<Click>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Click {
                button: PointerButton::Primary,
                hit: dummy_hit(),
                duration: Duration::from_millis(80),
            },
        ),
        chart,
    );
    app.update();

    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert_eq!(
            state.translation,
            Vec2::ZERO,
            "double-click reset で translation が Vec2::ZERO に戻るはず: {:?}",
            state.translation
        );
        assert_eq!(
            state.cell_width, DEFAULT_CELL_WIDTH,
            "double-click reset で cell_width が DEFAULT_CELL_WIDTH に戻るはず: {}",
            state.cell_width
        );
        assert!(
            state.auto_scale,
            "double-click reset で auto_scale が true に戻るはず"
        );
    }

    // ── Case 4: ドラッグ直後の click は double-click 列に入らない ──
    // drag してから click が来ても reset しない（pan 2 連発の誤検出ガード）。
    {
        let mut s = app
            .world_mut()
            .get_mut::<ChartViewState>(chart)
            .unwrap();
        s.translation = Vec2::new(10.0, 0.0);
        s.auto_scale = false;
    }

    // 左ドラッグ → dragged 印が ChartClickState に入る。
    app.world_mut().trigger_targets(
        Pointer::<Drag>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
                delta: Vec2::ZERO, // delta=0 なので translation は動かない
            },
        ),
        chart,
    );

    // drag 由来の click 1 回目 → dragged フラグを除去して last_click もクリア。
    app.world_mut().trigger_targets(
        Pointer::<Click>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Click {
                button: PointerButton::Primary,
                hit: dummy_hit(),
                duration: Duration::from_millis(50),
            },
        ),
        chart,
    );
    app.update();

    {
        // drag 由来 click では reset_view を呼ばない → auto_scale は false のまま。
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert!(
            !state.auto_scale,
            "drag 由来 click は reset_view を呼ばない。auto_scale は false のまま"
        );
    }

    // drag 由来 click の直後に genuine click 2 連発してもリセットされない
    // （drag 由来 click で last_click がクリアされたため、次の click が「1 発目」になる）。
    {
        let mut s = app
            .world_mut()
            .get_mut::<ChartViewState>(chart)
            .unwrap();
        s.translation = Vec2::new(5.0, 0.0); // reset してないので平行移動状態
    }
    // genuine click 1 回のみ（last_click は空なので double-click 未成立）。
    app.world_mut().trigger_targets(
        Pointer::<Click>::new(
            PointerId::Mouse,
            dummy_location(),
            chart,
            Click {
                button: PointerButton::Primary,
                hit: dummy_hit(),
                duration: Duration::from_millis(80),
            },
        ),
        chart,
    );
    app.update();

    {
        // 1 click だけでは reset されない。
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert_eq!(
            state.translation,
            Vec2::new(5.0, 0.0),
            "1 click だけでは reset されない: {:?}",
            state.translation
        );
    }
}
