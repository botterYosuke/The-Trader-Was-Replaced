use bevy::prelude::*;
use crate::ui::components::{WindowRoot, TitleBar, PriceDisplay, WindowManager};
use crate::ui::button::spawn_button;
use crate::ui::components::TradeButton;

pub fn setup_ui(mut commands: Commands) {
    spawn_trader_window(&mut commands, Vec2::new(0.0, 0.0));
    spawn_trader_window(&mut commands, Vec2::new(450.0, 100.0));
}

pub fn spawn_trader_window(commands: &mut Commands, position: Vec2) {
    // Window Root
    let window_id = commands.spawn((
        Sprite {
            color: Color::srgba(0.05, 0.05, 0.1, 0.95),
            custom_size: Some(Vec2::new(400.0, 500.0)),
            ..default()
        },
        Transform::from_xyz(position.x, position.y, 10.0),
        WindowRoot,
    ))
    .observe(|trigger: Trigger<Pointer<Down>>, mut query: Query<&mut Transform, With<WindowRoot>>, mut wm: ResMut<WindowManager>| {
        // Bring to front
        wm.max_z += 1.0;
        if let Ok(mut transform) = query.get_mut(trigger.entity()) {
            transform.translation.z = 10.0 + wm.max_z;
        }
    })
    .id();

    // Rim Light / Border
    commands.spawn((
        Sprite {
            color: Color::srgba(0.0, 0.8, 1.0, 0.3),
            custom_size: Some(Vec2::new(404.0, 504.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -0.1),
    )).set_parent(window_id);

    // Title Bar (Draggable area)
    let title_bar = commands.spawn((
        Sprite {
            color: Color::srgba(0.1, 0.1, 0.2, 1.0),
            custom_size: Some(Vec2::new(400.0, 40.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 230.0, 0.1),
        TitleBar,
    ))
    .observe(|drag: Trigger<Pointer<Drag>>, mut query: Query<&mut Transform, With<WindowRoot>>, parent_query: Query<&Parent>| {
        if let Ok(parent) = parent_query.get(drag.entity()) {
            if let Ok(mut transform) = query.get_mut(parent.get()) {
                transform.translation.x += drag.event().delta.x;
                transform.translation.y -= drag.event().delta.y;
            }
        }
    })
    .id();

    // Title Text
    let title_text = commands.spawn((
        Text2d::new("TRADER DASHBOARD"),
        TextFont { font_size: 20.0, ..default() },
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, 0.1),
    )).id();
    commands.entity(title_bar).add_child(title_text);

    // Price Display
    let price_text = commands.spawn((
        Text2d::new("$100.00"),
        TextFont { font_size: 60.0, ..default() },
        TextColor(Color::srgb(0.0, 1.0, 0.5)),
        Transform::from_xyz(0.0, 120.0, 0.1),
        PriceDisplay,
    )).id();

    // Buttons
    let buy_button = spawn_button(commands, "BUY", Color::srgb(0.0, 0.8, 0.4), Vec2::new(-80.0, -180.0), TradeButton::Buy);
    let sell_button = spawn_button(commands, "SELL", Color::srgb(0.8, 0.2, 0.2), Vec2::new(80.0, -180.0), TradeButton::Sell);

    commands.entity(window_id).add_child(title_bar);
    commands.entity(window_id).add_child(price_text);
    commands.entity(window_id).add_child(buy_button);
    commands.entity(window_id).add_child(sell_button);
}
