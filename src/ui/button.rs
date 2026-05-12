use bevy::prelude::*;
use crate::ui::components::TradeButton;
use crate::trading::TradingData;

pub fn spawn_button(
    commands: &mut Commands,
    label: &str,
    color: Color,
    position: Vec2,
    action: TradeButton,
) -> Entity {
    let btn = commands.spawn((
        Sprite {
            color,
            custom_size: Some(Vec2::new(120.0, 50.0)),
            ..default()
        },
        Transform::from_xyz(position.x, position.y, 0.1),
        action,
    ))
    .observe(move |_: Trigger<Pointer<Click>>, mut data: ResMut<TradingData>| {
        match action {
            TradeButton::Buy => data.price += 1.5,
            TradeButton::Sell => data.price -= 1.5,
        }
    })
    .id();

    let text = commands.spawn((
        Text2d::new(label),
        TextFont { font_size: 24.0, ..default() },
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, 0.1),
    )).id();

    commands.entity(btn).add_child(text);
    btn
}
