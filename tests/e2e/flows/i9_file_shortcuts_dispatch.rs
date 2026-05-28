//! I9 file_shortcuts_dispatch — Ctrl+S / Ctrl+Shift+S / Ctrl+O がそれぞれ Save / Save As / Open の
//! layout event を 1 回ずつ発火し、500ms クールダウン中はキーリピートで多重発火しないことを保証する（kind:ui）。
//!
//! テストでは本番 `layout_shortcut_system` を駆動し、`LayoutSaveRequested` / `LayoutSaveAsRequested` /
//! `LayoutLoadDialogRequested` の発火数を観測する。
//!
//! `layout_shortcut_system` の cooldown は `Local<f32>` を `time.delta_secs()` で減算する。
//! bare `App` の `Time<()>` は `advance_by` した値で delta が固定されるため、
//! コンボ間は `advance_by(1s)` で cooldown を確実に解除し、cooldown 持続の検証では
//! `advance_by(ZERO)` で delta=0 にして「時間が進まないキーリピート」を再現する。

use std::time::Duration;

use bevy::prelude::*;

use backcast::ui::layout_persistence::{
    layout_shortcut_system, LayoutLoadDialogRequested, LayoutSaveAsRequested, LayoutSaveRequested,
};

#[test]
fn i9_file_shortcuts_dispatch() {
    let mut app = App::new();

    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(Time::<()>::default());
    app.add_message::<LayoutSaveRequested>();
    app.add_message::<LayoutSaveAsRequested>();
    app.add_message::<LayoutLoadDialogRequested>();
    app.add_systems(Update, layout_shortcut_system);

    // 1 秒進めて cooldown を解除しつつ、コンボを just_pressed で押し直して 1 フレーム回す。
    let press_combo = |app: &mut App, combo: &[KeyCode]| {
        app.world_mut()
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(1));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        for &k in combo {
            keys.press(k);
        }
        app.update();
    };

    // ── Ctrl+S → LayoutSaveRequested が 1 回 ──
    press_combo(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert_eq!(
        app.world_mut()
            .resource_mut::<Messages<LayoutSaveRequested>>()
            .drain()
            .count(),
        1,
        "Ctrl+S で LayoutSaveRequested が 1 回発火するはず"
    );

    // ── Ctrl+Shift+S → LayoutSaveAsRequested が 1 回（Save ではなく Save As）──
    press_combo(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyS],
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<Messages<LayoutSaveAsRequested>>()
            .drain()
            .count(),
        1,
        "Ctrl+Shift+S で LayoutSaveAsRequested が 1 回発火するはず"
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<Messages<LayoutSaveRequested>>()
            .drain()
            .count(),
        0,
        "Ctrl+Shift+S は Save ではなく Save As のはず"
    );

    // ── Ctrl+O → LayoutLoadDialogRequested が 1 回（cooldown が 0.5 にセットされる）──
    press_combo(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyO]);
    assert_eq!(
        app.world_mut()
            .resource_mut::<Messages<LayoutLoadDialogRequested>>()
            .drain()
            .count(),
        1,
        "Ctrl+O で LayoutLoadDialogRequested が 1 回発火するはず"
    );

    // ── cooldown 持続: 時間を進めず（delta=0）Ctrl+O 押下が継続しても再発火しない ──
    // キーは reset せず、just_pressed が残った「OS キーリピート」状態を再現する。
    app.world_mut()
        .resource_mut::<Time<()>>()
        .advance_by(Duration::ZERO);
    app.update();
    assert_eq!(
        app.world_mut()
            .resource_mut::<Messages<LayoutLoadDialogRequested>>()
            .drain()
            .count(),
        0,
        "500ms クールダウン中（時間未経過）は多重発火しないはず"
    );
}
