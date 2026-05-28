//! Two `CodeEditor` entities side-by-side, each with its own state.
//!
//! Validates that the editor crate is genuinely multi-instance:
//!   - Two editors coexist without panicking on `single()` queries.
//!   - Each tracks its own cursor, selection, fold regions, and bracket
//!     match state.
//!   - Click-to-focus routes keyboard input to the editor under the cursor
//!     via `bevy::input_focus::InputFocus`.
//!
//! Layout: the window is split horizontally; the left editor renders on
//! `RenderLayers::layer(0)`, the right editor on `RenderLayers::layer(1)`.
//! Each editor has its own viewport rect and dedicated 2D camera; mouse
//! hit-testing maps screen coordinates back to the editor whose viewport
//! contains them.
//!
//! Run: `cargo run --example editor_multi`

use bevscode::prelude::*;
use bevy::prelude::*;
use bevy_camera::visibility::RenderLayers;

const WINDOW_WIDTH: f32 = 1600.0;
const WINDOW_HEIGHT: f32 = 900.0;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "bevscode — multi-editor".into(),
                        resolution: [WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32].into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: "assets".into(),
                    ..default()
                }),
        )
        .add_plugins(CodeEditorPlugins)
        .add_systems(Startup, spawn_two_editors)
        .run();
}

fn spawn_two_editors(
    windows: Query<&Window>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
) {
    let Ok(window) = windows.single() else { return };
    // Camera::viewport rects are in physical pixels; Node size is in logical pixels.
    let scale = window.scale_factor();
    let phys_w = (window.width() * scale) as u32;
    let phys_h = (window.height() * scale) as u32;
    let phys_half = phys_w / 2;
    let log_half = phys_half as f32 / scale;
    let log_h = window.height();

    let font =
        TextFont::from_font_size(14.0).with_font(asset_server.load("fonts/FiraMono-Regular.ttf"));
    let faces = MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf"));

    let left_layer = RenderLayers::layer(0);
    let right_layer = RenderLayers::layer(1);

    commands.spawn((
        Camera2d,
        Camera {
            order: 0,
            viewport: Some(bevy::camera::Viewport {
                physical_position: UVec2::new(0, 0),
                physical_size: UVec2::new(phys_half, phys_h),
                ..default()
            }),
            ..default()
        },
        left_layer.clone(),
        Name::new("LeftCamera"),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            viewport: Some(bevy::camera::Viewport {
                physical_position: UVec2::new(phys_half, 0),
                physical_size: UVec2::new(phys_w - phys_half, phys_h),
                ..default()
            }),
            ..default()
        },
        right_layer.clone(),
        Name::new("RightCamera"),
    ));

    let left = commands
        .spawn((
            CodeEditor,
            font.clone(),
            faces.clone(),
            Node {
                width: Val::Px(log_half),
                height: Val::Px(log_h),
                ..default()
            },
            left_layer,
            Name::new("LeftEditor"),
        ))
        .id();
    let right = commands
        .spawn((
            CodeEditor,
            font,
            faces,
            Node {
                width: Val::Px(window.width() - log_half),
                height: Val::Px(log_h),
                ..default()
            },
            right_layer,
            Name::new("RightEditor"),
        ))
        .id();

    input_focus.set(left);

    info!(
        "Spawned multi-editor demo: left = {:?}, right = {:?}. Click either side to focus.",
        left, right
    );
}
