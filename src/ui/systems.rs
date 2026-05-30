use crate::trading::{BackendStatus, LastPrices, SelectedSymbol, TradingSettings};
use crate::ui::components::{PriceDisplay, StatusIndicator};
use crate::ui::theme::Theme;
use bevy::prelude::*;

pub fn update_price_display(
    last_prices: Res<LastPrices>,
    selected_symbol: Res<SelectedSymbol>,
    mut query: Query<&mut Text2d, With<PriceDisplay>>,
) {
    let price = selected_symbol
        .id
        .as_ref()
        .and_then(|id| last_prices.map.get(id));
    let label = match price {
        Some(p) => format!("${:.2}", p),
        None => "$--".to_string(),
    };
    for mut text in query.iter_mut() {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

pub fn update_status_indicator(
    status: Res<BackendStatus>,
    settings: Res<TradingSettings>,
    theme: Res<Theme>,
    mut query: Query<&mut Sprite, With<StatusIndicator>>,
) {
    if !settings.backend_enabled {
        for mut sprite in query.iter_mut() {
            sprite.color = theme.colors.element_disabled;
        }
        return;
    }

    let color = if status.connected {
        if status.running {
            theme.status.success
        } else {
            theme.status.warning
        }
    } else {
        theme.status.error
    };

    for mut sprite in query.iter_mut() {
        sprite.color = color;
    }
}
