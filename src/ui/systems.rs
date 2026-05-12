use bevy::prelude::*;
use bevy_vector_shapes::prelude::*;
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
    mut painter: ShapePainter,
    data: Res<TradingData>,
    window_query: Query<&Transform, With<WindowRoot>>,
) {
    painter.set_2d();
    if data.history.len() < 2 { return; }

    for transform in &window_query {
        painter.set_translation(Vec3::ZERO);
        let base_pos = transform.translation.truncate() + Vec2::new(-180.0, -50.0);
        let z = transform.translation.z + 0.1;

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

        // Draw smooth lines with ShapePainter
        painter.thickness = 3.0;
        painter.color = Color::srgb(0.0, 0.8, 1.0);
        
        for window in points.windows(2) {
            painter.line(
                Vec3::new(window[0].x, window[0].y, z),
                Vec3::new(window[1].x, window[1].y, z),
            );
        }
        
        // Add "Wow" factor: Glowing end point
        if let Some(&last) = points.last() {
            painter.set_translation(Vec3::new(last.x, last.y, z + 0.1));
            painter.color = Color::WHITE;
            painter.circle(4.0);
            
            painter.color = Color::srgba(0.0, 0.8, 1.0, 0.4);
            painter.circle(8.0);
        }
    }
}
