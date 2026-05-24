//! K4 chart_ctrl_wheel_camera_zoom — Ctrl+wheel は PanCam の camera zoom だけを変え、
//! chart 自身の価格スケールを変更しないことを保証する（kind:ui）。
//!
//! ## 設計
//! `chart_scroll_zoom_system` の先頭で:
//! ```ignore
//! if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
//!     wheel.clear(); // Ctrl+wheel はカメラズームに委譲。stale event を残さず捨てる
//!     return;
//! }
//! ```
//! と early-return する。この挙動を 2 通りで確認する：
//!
//! 1. **Ctrl なし + hover なし** — ホイールイベントは消費されるが、HoverMap にエントリが無いため
//!    `apply_cursor_zoom` には届かず `ChartViewState` は変わらない。
//! 2. **Ctrl あり** — early-return によりホイールイベントが `wheel.clear()` で破棄され、
//!    `ChartViewState` は変わらない。
//!
//! camera zoom 側（PanCam / `OrthographicProjection.scale`）は bevy_pancam プラグインが
//! `Ctrl+wheel` で書き換えるが、bevy_pancam は headless では動かせない（Window + Render 依存）。
//! camera scale の**変化**は PanCam の責務であり本テストのスコープ外; 本テストは
//! 「Ctrl+wheel で chart zoom が *適用されない*」否定命題を headless で確認する。
//!
//! PanCam の動作検証が必要な場合は kind:render の smoke test（L4）で行う。

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

use backcast::ui::chart_interaction::chart_scroll_zoom_system;
use backcast::ui::chart_viewstate::ChartViewState;
use backcast::ui::components::ChartInstrument;

#[test]
fn k4_chart_ctrl_wheel_camera_zoom() {
    let mut app = App::new();
    app.add_plugins(bevy::app::TaskPoolPlugin::default());
    app.add_plugins(bevy::app::ScheduleRunnerPlugin::default());
    app.add_plugins(bevy::time::TimePlugin);

    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(HoverMap::default());
    app.add_event::<MouseWheel>();

    let chart = app
        .world_mut()
        .spawn((
            ChartViewState::default(),
            ChartInstrument {
                instrument_id: "9984.TSE".to_string(),
            },
            GlobalTransform::default(),
        ))
        .id();

    app.add_systems(Update, chart_scroll_zoom_system);

    let initial_cell_width = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .cell_width;
    let initial_auto_scale = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .auto_scale;

    // ── Case 1: Ctrl なし + hover なし — zoom は適用されない ──
    // HoverMap が空なので system は hover ループを素通りし、ChartViewState は不変。
    app.world_mut().send_event(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 3.0,
        window: Entity::PLACEHOLDER,
    });
    app.update();

    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert_eq!(
            state.cell_width, initial_cell_width,
            "Ctrl なし + hover なし → cell_width 不変"
        );
        assert_eq!(
            state.auto_scale, initial_auto_scale,
            "Ctrl なし + hover なし → auto_scale 不変"
        );
    }

    // ── Case 2: Ctrl+wheel — early-return により chart zoom が適用されない ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
    }
    app.world_mut().send_event(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 5.0, // 大きい値でも chart zoom は適用されない
        window: Entity::PLACEHOLDER,
    });
    app.update();

    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert_eq!(
            state.cell_width, initial_cell_width,
            "Ctrl+wheel では chart cell_width が変化しないはず (camera に委譲)"
        );
        let initial_cell_height = ChartViewState::default().cell_height;
        assert!(
            (state.cell_height - initial_cell_height).abs() < 1e-6,
            "Ctrl+wheel では chart cell_height が変化しないはず: got {}",
            state.cell_height
        );
        assert_eq!(
            state.translation,
            Vec2::ZERO,
            "Ctrl+wheel では chart translation が変化しないはず"
        );
        assert!(
            state.auto_scale,
            "Ctrl+wheel では auto_scale が false になるべきでない (zoom 未適用)"
        );
    }

    // ── Case 3: Ctrl+Right でも同様 ──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ControlRight);
    }
    app.world_mut().send_event(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: -2.0,
        window: Entity::PLACEHOLDER,
    });
    app.update();

    {
        let state = app.world().entity(chart).get::<ChartViewState>().unwrap();
        assert_eq!(
            state.cell_width, initial_cell_width,
            "ControlRight+wheel でも chart cell_width が変化しないはず"
        );
    }

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}
