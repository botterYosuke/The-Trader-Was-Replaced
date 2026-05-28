use crate::ui::components::{LayoutExcluded, PanelKind};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;

pub fn spawn_settings_panel(commands: &mut Commands) {
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "SETTINGS".to_string(),
            size: Vec2::new(300.0, 140.0),
            position: Vec2::new(0.0, 0.0),
            accent: Color::srgb(0.50, 0.70, 1.0),
            closeable: true,
            resizable: false,
        },
    );
    commands.entity(root).insert((PanelKind::Settings, LayoutExcluded));
    commands.entity(content_area).with_children(|p| {
        p.spawn((
            Text::new("Theme: Dark\nBackend: localhost:19876\nSave Layout: —"),
            TextFont { font_size: 11.0, ..default() },
            TextColor(Color::srgb(0.75, 0.75, 0.85)),
            Node { padding: UiRect::all(Val::Px(8.0)), ..default() },
        ));
    });
}
