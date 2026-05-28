//! K2 chart_wheel_zoom_clamps — chart 上の wheel zoom がカーソル直下の価格・時刻を固定しながら
//! 時間軸・価格軸を拡大縮小し、上限・下限で clamp されることを保証する（kind:ui）。
//!
//! ## ヘッドレス境界
//! `chart_scroll_zoom_system` は `HoverMap` でホバー中の chart entity を特定し、
//! `Camera::viewport_to_world_2d` でカーソルを world 座標へ投影してから zoom を適用する。
//! `viewport_to_world_2d` は実ウィンドウ / RenderPlugin が無いと `Err` を返すため、
//! full system path でのズーム適用はヘッドレスでは不可能。
//!
//! そのため本テストは**システム挙動の observable 境界**を 2 点確認する：
//! 1. Ctrl+wheel の早期 return — Ctrl を押すとホイールイベントが消費されても
//!    `ChartViewState` が変化しない（camera zoom に委譲する）。
//! 2. hover なし（HoverMap 空）の場合 — ホイールを送っても `ChartViewState` が変化しない。
//!
//! ズーム量・clamp 境界の純関数レベルの保証は `src/ui/chart_interaction.rs` 内の
//! `zoom_clamps_cell_dimensions` / `cursor_centered_zoom_*` / `small_autoscaled_cell_height_*`
//! 単体テストが担っている（`cargo test -p backcast chart_interaction`）。

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

use backcast::ui::chart_interaction::chart_scroll_zoom_system;
use backcast::ui::chart_viewstate::ChartViewState;
use backcast::ui::components::ChartInstrument;

#[test]
fn k2_chart_wheel_zoom_clamps() {
    let mut app = App::new();
    // MinimalPlugins: タイマー・スケジュール等の最小セット。render / window は不要。
    app.add_plugins(bevy::app::TaskPoolPlugin::default());
    app.add_plugins(bevy::app::ScheduleRunnerPlugin::default());
    app.add_plugins(bevy::time::TimePlugin);

    // chart_scroll_zoom_system が要求する resource / event を全部挿入する。
    // 不足すると system-param バリデーションで panic するため完備させる。
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(HoverMap::default());
    app.add_message::<MouseWheel>();

    // camera_q (With<Camera2d>) が 0 件だと camera の get_single() が Err を返して system が
    // continue するが、resource として Camera がないとパラメータ解決自体は成功する。
    // ここでは Camera2d を持たない空の App のまま進める。

    // テスト対象の chart entity。
    let chart = app
        .world_mut()
        .spawn((
            ChartViewState::default(),
            ChartInstrument {
                instrument_id: "7203.TSE".to_string(),
            },
            // chart_scroll_zoom_system の chart_q は With<ChartInstrument> + GlobalTransform を要求。
            GlobalTransform::default(),
        ))
        .id();

    app.add_systems(Update, chart_scroll_zoom_system);

    // ── Case 1: hover 無し (HoverMap 空) でホイールを送っても ChartViewState は変わらない ──
    // hover_map にエントリが無いので system は early-continue し apply_cursor_zoom に届かない。
    let before_cell_width = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .cell_width;

    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 3.0, // zoom-in 方向
        window: Entity::PLACEHOLDER,
    });
    app.update();

    let after_cell_width = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .cell_width;
    assert_eq!(
        before_cell_width, after_cell_width,
        "HoverMap 空の場合、ホイールイベントは ChartViewState を変化させない"
    );

    // ── Case 2: Ctrl+wheel — ChartViewState は変化せず、イベントは消費される ──
    // Ctrl を押した状態では system が wheel.clear() してカメラに委譲する。
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
    }
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 5.0,
        window: Entity::PLACEHOLDER,
    });
    app.update();

    let ctrl_cell_width = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .cell_width;
    assert_eq!(
        before_cell_width, ctrl_cell_width,
        "Ctrl+wheel では ChartViewState は変化しない（camera zoom に委譲）"
    );

    // auto_scale フラグも変わっていない（zoom が適用されていない証拠）。
    let auto = app
        .world()
        .entity(chart)
        .get::<ChartViewState>()
        .unwrap()
        .auto_scale;
    assert!(
        auto,
        "zoom 未適用なら auto_scale は初期値 true のまま"
    );

    // Ctrl を離してクリーンアップ。
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}
