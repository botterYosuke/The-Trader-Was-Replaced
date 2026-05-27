//! Minimal editable text field — proves `bevy_instanced_text_editor` works
//! standalone, without `bevscode`'s IDE features.
//!
//! Spawns one [`TextEditor`] entity with the engine's GPU rendering, plus
//! [`InstancedTextEditPlugin`] which gives you typed-character insertion, cursor
//! movement (via the editing-event observers), drag selection, scroll, and
//! clipboard copy out of the box.
//!
//! What you DON'T get here: line numbers, multi-cursor, folding, brackets,
//! syntax highlighting, LSP. That's the point — those live in the editor
//! crate one tier up.

use bevy::prelude::*;
use bevy::text::LineHeight;
use bevy::text::TextFont;
use bevy_instanced_text::prelude::*;
use bevy_instanced_text_editor::{InstancedTextEditPlugin, TextEditor};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "bevy_instanced_text_editor — simple text input".to_string(),
                        resolution: (800u32, 400u32).into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: "assets".into(),
                    ..default()
                }),
        )
        .add_plugins(InstancedTextPlugins)
        .add_plugins(InstancedTextEditPlugin::default())
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);

    commands.spawn((
        TextEditor,
        TextFont::from_font_size(20.0).with_font(asset_server.load("fonts/FiraMono-Regular.ttf")),
        LineHeight::RelativeToFont(1.4),
        MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf")),
        Name::new("simple-text-input"),
    ));
}
