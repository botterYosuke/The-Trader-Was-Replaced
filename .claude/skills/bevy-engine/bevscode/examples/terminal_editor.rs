//! Side-by-side: a `CodeEditor` on the left, a `BevyTerminal` on the
//! right, separated by a thin 1-px divider.
//!
//! Each pane gets its own `Camera2d` with a `viewport` rect so they
//! occupy non-overlapping halves of the window. `RenderLayers` keeps
//! each view's draw calls routed to the correct camera.
//!
//! Run with: `cargo run --example terminal_editor`

use bevscode::prelude::*;
use bevsterm::prelude::*;
use bevy::prelude::*;
use bevy_camera::visibility::RenderLayers;

const DIVIDER_PX: u32 = 1;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "bevsterm — editor + terminal".into(),
                        resolution: [1280u32, 720u32].into(),
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
        .add_plugins(TerminalPlugin)
        .add_plugins(bevsterm::TerminalPtyPlugin)
        .add_systems(Startup, layout_panes)
        .add_systems(PostStartup, setup_editor_content)
        .run();
}

fn layout_panes(
    asset_server: Res<AssetServer>,
    windows: Query<&Window>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
    mut commands: Commands,
) {
    let Ok(window) = windows.single() else { return };
    // Physical pixels for camera viewport rects.
    let scale = window.scale_factor();
    let phys_w = (window.width() * scale) as u32;
    let phys_h = (window.height() * scale) as u32;
    let phys_divider = (DIVIDER_PX as f32 * scale) as u32;
    let phys_half = (phys_w - phys_divider) / 2;

    // Logical dimensions for Node size (used by Bevy UI layout).
    let log_half = phys_half as f32 / scale;
    let log_h = window.height();

    let bg = EditorTheme::default().background;
    let editor_layer = RenderLayers::layer(0);
    let terminal_layer = RenderLayers::layer(1);

    let font =
        TextFont::from_font_size(14.0).with_font(asset_server.load("fonts/FiraMono-Regular.ttf"));
    let faces = MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf"));

    // Left camera → editor.
    commands.spawn((
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(bg),
            viewport: Some(bevy::camera::Viewport {
                physical_position: UVec2::new(0, 0),
                physical_size: UVec2::new(phys_half, phys_h),
                ..default()
            }),
            ..default()
        },
        editor_layer.clone(),
        Name::new("EditorCamera"),
    ));

    // Right camera → terminal.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(bg),
            viewport: Some(bevy::camera::Viewport {
                physical_position: UVec2::new(phys_half + phys_divider, 0),
                physical_size: UVec2::new(phys_w - phys_half - phys_divider, phys_h),
                ..default()
            }),
            ..default()
        },
        terminal_layer.clone(),
        Name::new("TerminalCamera"),
    ));

    // 1-px divider drawn in NDC by a thin Sprite on its own layer.
    // We place it in the default camera (layer 0) at the window center.
    let divider_layer = RenderLayers::layer(2);
    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        divider_layer.clone(),
        Name::new("DividerCamera"),
    ));
    let divider_color = Color::srgba(0.3, 0.3, 0.3, 1.0);
    commands.spawn((
        Sprite {
            color: divider_color,
            custom_size: Some(Vec2::new(DIVIDER_PX as f32, window.height())),
            ..default()
        },
        Transform::from_xyz(
            // half_w − window_half puts the divider at the left/right boundary.
            log_half - window.width() / 2.0,
            0.0,
            0.0,
        ),
        divider_layer,
        Name::new("PaneDivider"),
    ));

    let editor = commands
        .spawn((
            CodeEditor,
            font.clone(),
            faces.clone(),
            Node {
                width: Val::Px(log_half),
                height: Val::Px(log_h),
                ..default()
            },
            editor_layer,
            Name::new("Editor"),
        ))
        .id();
    input_focus.set(editor);

    // Terminal — right pane.
    commands.spawn((
        BevyTerminal,
        font,
        faces,
        Node {
            width: Val::Px(window.width() - log_half),
            height: Val::Px(log_h),
            padding: UiRect::left(Val::Px(12.0)),
            ..default()
        },
        terminal_layer,
        Name::new("Terminal"),
    ));
}

fn setup_editor_content(
    mut commands: Commands,
    editor_query: Query<Entity, With<CodeEditor>>,
    mut set_text_writer: MessageWriter<SetTextRequested>,
) {
    let Ok(entity) = editor_query.single() else {
        return;
    };

    let text = r#"use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text::new("Hello, Bevy!"),
        TextFont {
            font_size: 64.0,
            ..default()
        },
        TextColor(Color::WHITE),
    ));
}
"#;

    set_text_writer.write(SetTextRequested {
        entity,
        text: text.to_string(),
    });

    commands.entity(entity).insert(TreeSitterGrammar::new(
        bevy_tree_sitter::arborium::lang_rust::language().into(),
        bevy_tree_sitter::arborium::lang_rust::HIGHLIGHTS_QUERY,
    ));
}
