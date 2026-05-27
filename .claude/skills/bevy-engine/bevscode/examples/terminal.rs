#![allow(clippy::too_many_arguments)]

//! Single-pane terminal demo. Spawns a `BevyTerminal`, fills the
//! window. Type into it; resize it; drag-select; Cmd+C / Cmd+V (or
//! Ctrl+Shift+C / Ctrl+Shift+V on Linux/Windows).
//!
//! Run with: `cargo run -p bevsterm --example basic_terminal`

use bevsterm::prelude::*;
use bevy::prelude::*;

#[cfg(feature = "profile")]
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "bevsterm — basic".into(),
                    resolution: [960u32, 600u32].into(),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::asset::AssetPlugin {
                file_path: "assets".into(),
                ..default()
            }),
    )
    .add_plugins(TerminalPlugins)
    .add_systems(Startup, (setup_camera, spawn_terminal))
    .add_systems(Update, log_events);

    #[cfg(feature = "profile")]
    app.add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_systems(Startup, spawn_perf_overlay)
        .add_systems(Update, update_perf_overlay);

    app.run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn spawn_terminal(mut commands: Commands, asset_server: Res<AssetServer>, windows: Query<&Window>) {
    let Ok(window) = windows.single() else { return };
    let w = window.width();
    let h = window.height();

    let regular: Handle<bevy::text::Font> = asset_server.load("fonts/FiraMono-Regular.ttf");
    let bold: Handle<bevy::text::Font> = asset_server.load("fonts/FiraMono-Medium.ttf");

    commands.spawn((
        BevyTerminal,
        TextFont::from_font_size(14.0).with_font(regular),
        MonoFontFaces::default().with_bold(bold),
        // Val::Px so Bevy UI layout can resolve size without a UI camera.
        Node {
            width: Val::Px(w),
            height: Val::Px(h),
            padding: UiRect::left(Val::Px(12.0)),
            ..default()
        },
    ));

    info!("Spawned BevyTerminal");
}

fn log_events(
    mut ready: MessageReader<TerminalReady>,
    mut titles: MessageReader<TerminalTitleChanged>,
    mut bells: MessageReader<TerminalBell>,
    mut exits: MessageReader<TerminalExited>,
    mut cwd: MessageReader<TerminalCwdChanged>,
    mut finished: MessageReader<TerminalBlockFinished>,
    mut selected: MessageReader<TerminalBlockSelected>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
) {
    for ev in ready.read() {
        info!("ready({:?}): {}x{}", ev.entity, ev.cols, ev.rows);
        input_focus.set(ev.entity);
    }
    for ev in titles.read() {
        info!("title({:?}): {:?}", ev.entity, ev.title);
    }
    for ev in bells.read() {
        info!("bell({:?})", ev.entity);
    }
    for ev in exits.read() {
        info!("exit: {:?}", ev);
    }
    for ev in cwd.read() {
        info!("cwd({:?}): {}", ev.entity, ev.cwd);
    }
    for ev in finished.read() {
        info!(
            "block_finished({:?}): id={} exit={:?}",
            ev.entity, ev.block_id, ev.exit_code
        );
    }
    for ev in selected.read() {
        info!("block_selected({:?}): id={}", ev.entity, ev.block_id);
    }
}

#[cfg(feature = "profile")]
#[derive(Component)]
struct PerfOverlay;

#[cfg(feature = "profile")]
fn spawn_perf_overlay(mut commands: Commands, windows: Query<&Window>) {
    let Ok(window) = windows.single() else {
        return;
    };
    let x = window.width() / 2.0 - 12.0;
    let y = window.height() / 2.0 - 12.0;

    commands.spawn((
        PerfOverlay,
        Text2d::new("fps: -"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.9, 0.2)),
        bevy::sprite::Anchor::TOP_RIGHT,
        Transform::from_xyz(x, y, 1000.0),
    ));
}

#[cfg(feature = "profile")]
fn update_perf_overlay(
    diagnostics: Res<DiagnosticsStore>,
    mut overlay: Query<&mut Text2d, With<PerfOverlay>>,
) {
    let Ok(mut text) = overlay.single_mut() else {
        return;
    };
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    text.0 = format!("fps {fps:>5.1}   frame {frame_ms:>5.2} ms");
}
