use bevy::prelude::*;
use crate::trading::TradingData;
use crate::ui::components::{PriceDisplay, WindowRoot};

pub fn ui_update_system(
    data: Res<TradingData>,
    mut query: Query<(&mut Text2d, &mut TextColor), With<PriceDisplay>>,
) {
    for (mut text, mut color) in &mut query {
        text.0 = format!("${:.2}", data.price);
        color.0 = if data.price >= 100.0 {
            Color::srgb(0.0, 1.0, 0.5)
        } else {
            Color::srgb(1.0, 0.2, 0.2)
        };
    }
}

pub fn chart_rendering_system(
    mut gizmos: Gizmos,
    data: Res<TradingData>,
    window_query: Query<&Transform, With<WindowRoot>>,
) {
    if data.history.len() < 2 { return; }

    for transform in &window_query {
        let base_pos = transform.translation.truncate() + Vec2::new(-180.0, -50.0);

        let max_price = data.history.iter().cloned().fold(f32::NEG_INFINITY, f32::max).max(105.0);
        let min_price = data.history.iter().cloned().fold(f32::INFINITY, f32::min).min(95.0);
        let range = (max_price - min_price).max(1.0);

        let x_step = 360.0 / (data.history.len() - 1) as f32;
        let mut points = Vec::new();

        for (i, &p) in data.history.iter().enumerate() {
            let x = i as f32 * x_step;
            let y = (p - min_price) / range * 150.0;
            points.push(base_pos + Vec2::new(x, y));
        }

        // Draw line segments with Gizmos
        for window in points.windows(2) {
            gizmos.line_2d(window[0], window[1], Color::srgb(0.0, 0.8, 1.0));
        }
        
        // Add some "Wow" factor: Glowing end point
        if let Some(&last) = points.last() {
            gizmos.circle_2d(last, 4.0, Color::srgb(1.0, 1.0, 1.0));
            gizmos.circle_2d(last, 8.0, Color::srgba(0.0, 0.8, 1.0, 0.4));
        }
    }
}
