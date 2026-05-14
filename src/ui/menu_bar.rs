use bevy::prelude::*;
use rfd::FileDialog;
use crate::ui::components::{MenuBarRoot, MenuButton, OpenStrategyRequested};

const BTN_NORMAL: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BTN_HOVER: Color = Color::srgba(0.20, 0.20, 0.30, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.30, 0.30, 0.48, 1.0);

fn spawn_menu_btn(parent: &mut ChildBuilder, label: &str, action: MenuButton) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BTN_NORMAL),
            action,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(label),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.82, 0.82, 0.82)),
            ));
        });
}

pub fn spawn_menu_bar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(24.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(2.0),
                padding: UiRect::horizontal(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.07, 0.11, 0.95)),
            MenuBarRoot,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("File"),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.65, 0.65, 0.65)),
            ));

            spawn_menu_btn(p, "Open Strategy...", MenuButton::OpenStrategy);
        });
}

pub fn menu_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &MenuButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut open_strategy_events: EventWriter<OpenStrategyRequested>,
) {
    for (interaction, mut bg, action) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                match action {
                    MenuButton::OpenStrategy => {
                        info!("menu: open strategy requested");
                        if let Some(path) = FileDialog::new()
                            .add_filter("Python strategy", &["py"])
                            .set_directory("python/tests/data")
                            .pick_file()
                        {
                            info!("menu: selected strategy: {:?}", path);
                            open_strategy_events.send(OpenStrategyRequested { path });
                        } else {
                            info!("menu: open strategy canceled");
                        }
                    }
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

pub fn log_open_strategy_requested_system(
    mut events: EventReader<OpenStrategyRequested>,
) {
    for event in events.read() {
        info!("open strategy selected: {:?}", event.path);
    }
}
