use crate::trading::{BackendStatus, TradingData, TradingSettings};
use crate::ui::components::{PriceDisplay, StatusIndicator};
use bevy::prelude::*;

pub fn update_price_display(
    data: Res<TradingData>,
    mut query: Query<&mut Text2d, With<PriceDisplay>>,
) {
    for mut text in query.iter_mut() {
        text.0 = format!("${:.2}", data.price);
    }
}

pub fn update_status_indicator(
    status: Res<BackendStatus>,
    settings: Res<TradingSettings>,
    mut query: Query<&mut Sprite, With<StatusIndicator>>,
) {
    if !settings.backend_enabled {
        for mut sprite in query.iter_mut() {
            sprite.color = Color::srgb(0.3, 0.3, 0.3); // Disabled: Dark Gray
        }
        return;
    }

    let color = if status.connected {
        if status.running {
            Color::srgb(0.0, 1.0, 0.0) // Connected & Running: Green
        } else {
            Color::srgb(1.0, 1.0, 0.0) // Connected but Paused: Yellow
        }
    } else {
        Color::srgb(1.0, 0.0, 0.0) // Error/Disconnected: Red
    };

    for mut sprite in query.iter_mut() {
        sprite.color = color;
    }
}

pub fn button_system(
    mut interaction_query: Query<
        (
            &Interaction,
            &mut Sprite,
            &crate::ui::components::TradeButton,
        ),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut sprite, button_type) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                sprite.color = Color::srgb(1.0, 1.0, 1.0);
                match button_type {
                    crate::ui::components::TradeButton::Buy => info!("BUY pressed!"),
                    crate::ui::components::TradeButton::Sell => info!("SELL pressed!"),
                }
            }
            Interaction::Hovered => {
                sprite.color = Color::srgb(0.5, 0.5, 0.5);
            }
            Interaction::None => {
                sprite.color = match button_type {
                    crate::ui::components::TradeButton::Buy => Color::srgb(0.0, 0.8, 0.4),
                    crate::ui::components::TradeButton::Sell => Color::srgb(0.8, 0.2, 0.2),
                };
            }
        }
    }
}
