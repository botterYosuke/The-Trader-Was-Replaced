//! Screenshot variant of `editor_perf` for visual regression bisecting.
//!
//! Sets up the same editor as `editor_perf`, waits a fixed number of frames
//! for layout/atlas/treesitter to converge, then captures the primary window
//! and saves a PNG. Exits after capture so it's safe to run in a script.
//!
//! Usage: `cargo run --example editor_perf_screenshot -- <out_path>`
//! Default output: `target/editor_perf_screenshot.png`.

use bevscode::prelude::*;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

#[derive(Resource)]
struct CaptureConfig {
    out_path: String,
    /// Frame index at which to take the shot. Earlier frames may still be
    /// shaping / loading.
    capture_at_frame: u64,
    /// Frame index at which to exit unconditionally.
    exit_at_frame: u64,
}

#[derive(Resource, Default)]
struct FrameCounter(u64);

fn main() {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/editor_perf_screenshot.png".to_string());

    let mut app = App::new();
    app.insert_resource(CaptureConfig {
        out_path,
        capture_at_frame: 180,
        exit_at_frame: 300,
    });
    app.init_resource::<FrameCounter>();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "editor_perf_screenshot".to_string(),
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
    .add_systems(Update, (tick_frames, capture_then_exit));

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
        Ok(content) => content,
        Err(e) => format!(
            "// Failed to load sqlite3.c: {}\n// Make sure assets/sqlite3.c exists",
            e
        ),
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

fn tick_frames(mut counter: ResMut<FrameCounter>) {
    counter.0 += 1;
}

fn capture_then_exit(
    mut commands: Commands,
    counter: Res<FrameCounter>,
    config: Res<CaptureConfig>,
    mut exit: MessageWriter<AppExit>,
    mut captured: Local<bool>,
) {
    if !*captured && counter.0 == config.capture_at_frame {
        let path = config.out_path.clone();
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        println!("[screenshot] capturing frame {} -> {}", counter.0, path);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        *captured = true;
    }
    if counter.0 >= config.exit_at_frame {
        println!("[screenshot] exiting after frame {}", counter.0);
        exit.write(AppExit::Success);
    }
}
