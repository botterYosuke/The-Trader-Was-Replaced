//! Performance test example with a very large file (sqlite3.c - 150k+ lines)
//!
//! This example loads sqlite3.c to test scrolling performance, viewport culling,
//! and entity pooling with a massive codebase.

use bevscode::prelude::*;
use bevy::prelude::*;
use bevy::window::{CursorIcon, SystemCursorIcon};

#[cfg(feature = "profile")]
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Bevy Code Editor - Performance Test (sqlite3.c - 150k lines)"
                        .to_string(),
                    resolution: (1400, 900).into(),
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
    .add_systems(Startup, (setup_camera, spawn_editor))
    .add_systems(PostStartup, setup_editor)
    .add_systems(Update, update_cursor_icon);

    #[cfg(feature = "profile")]
    app.add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_systems(Startup, spawn_perf_overlay)
        .add_systems(Update, update_perf_overlay);

    app.run();
}

fn spawn_editor(mut commands: Commands) {
    commands.spawn((CodeEditor, AutoResizeViewport, Name::new("CodeEditor")));
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(EditorTheme::default().background),
            ..default()
        },
    ));
}

fn setup_editor(
    mut commands: Commands,
    editor_query: Query<Entity, With<CodeEditor>>,
    asset_server: Res<AssetServer>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
    mut set_text_writer: MessageWriter<SetTextRequested>,
) {
    let Ok(entity) = editor_query.single() else {
        return;
    };

    commands.entity(entity).insert((
        TextFont::from_font_size(14.0).with_font(asset_server.load("fonts/FiraMono-Regular.ttf")),
        MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf")),
    ));

    input_focus.set(entity);

    let file_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/sqlite3.c");
    let content = match std::fs::read_to_string(&file_path) {
        Ok(content) => {
            println!(
                "Loaded {} with {} lines",
                file_path.display(),
                content.lines().count()
            );
            content
        }
        Err(e) => {
            eprintln!("Failed to load {}: {}", file_path.display(), e);
            format!(
                "// Failed to load sqlite3.c: {}\n// Make sure assets/sqlite3.c exists",
                e
            )
        }
    };

    set_text_writer.write(SetTextRequested {
        entity,
        text: content,
    });

    commands.entity(entity).insert(TreeSitterGrammar::new(
        bevy_tree_sitter::arborium::lang_c::language().into(),
        bevy_tree_sitter::arborium::lang_c::HIGHLIGHTS_QUERY,
    ));
}

fn update_cursor_icon(
    editor_query: Query<(Entity, &ComputedNode), With<CodeEditor>>,
    input_focus: Res<bevy::input_focus::InputFocus>,
    mut commands: Commands,
    windows: Query<(Entity, &Window), With<Window>>,
) {
    let Ok((editor_entity, computed)) = editor_query.single() else {
        return;
    };

    if let Ok((window_entity, window)) = windows.single() {
        let Some(cursor_pos) = window.cursor_position() else {
            return;
        };

        let cursor_x = cursor_pos.x - window.width() / 2.0;
        let viewport_width = computed.size().x * computed.inverse_scale_factor();
        let code_area_right = viewport_width / 2.0 - 20.0;

        let over_code = cursor_x < code_area_right;
        let is_focused = input_focus.get() == Some(editor_entity);
        let icon = if is_focused && over_code {
            CursorIcon::System(SystemCursorIcon::Text)
        } else {
            CursorIcon::System(SystemCursorIcon::Default)
        };
        commands.entity(window_entity).insert(icon);
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
