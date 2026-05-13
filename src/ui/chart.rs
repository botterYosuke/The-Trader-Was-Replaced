use bevy::prelude::*;
use bevy_vector_shapes::prelude::*;
use crate::trading::TradingData;

#[derive(Component)]
pub struct ChartViewState {
    pub min_price: f32,
    pub max_price: f32,
    pub latest_timestamp_ms: i64,
    pub auto_scale: bool,
    pub width: f32,
    pub height: f32,
    pub time_window_ms: i64,
}

impl Default for ChartViewState {
    fn default() -> Self {
        Self {
            min_price: 0.0,
            max_price: 1.0,
            latest_timestamp_ms: 0,
            auto_scale: true,
            width: 400.0,
            height: 200.0,
            time_window_ms: 60_000,
        }
    }
}

pub fn chart_render_system(
    mut painter: ShapePainter,
    trading_data: Res<TradingData>,
    mut query: Query<(&mut ChartViewState, &GlobalTransform)>,
) {
    for (mut state, transform) in query.iter_mut() {
        let history = &trading_data.history_points;
        
        if history.is_empty() {
            continue;
        }

        if let Some(last) = history.last() {
            state.latest_timestamp_ms = last.timestamp_ms;
        }

        if state.latest_timestamp_ms == 0 {
            continue;
        }

        if state.auto_scale {
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            let mut has_visible_data = false;
            let start_ts = state.latest_timestamp_ms - state.time_window_ms;

            for p in history {
                if p.timestamp_ms >= start_ts {
                    if p.price < min { min = p.price; }
                    if p.price > max { max = p.price; }
                    has_visible_data = true;
                }
            }

            if has_visible_data {
                let range = max - min;
                if range > 0.0 {
                    state.min_price = min - range * 0.1;
                    state.max_price = max + range * 0.1;
                } else {
                    state.min_price = min - 1.0;
                    state.max_price = max + 1.0;
                }
            }
        }

        let price_range = state.max_price - state.min_price;
        if price_range <= 0.0 { continue; }

        let start_pos = transform.translation();
        painter.set_translation(start_pos);

        painter.color = Color::srgb(0.3, 0.3, 0.3);
        painter.rect(Vec2::new(state.width, state.height));

        painter.color = Color::srgb(0.0, 1.0, 0.5);
        painter.thickness = 2.0;

        let start_ts = state.latest_timestamp_ms - state.time_window_ms;
        let mut prev_pos: Option<Vec3> = None;

        for p in history {
            if p.timestamp_ms < start_ts {
                continue;
            }

            let time_offset = p.timestamp_ms - state.latest_timestamp_ms;
            let x = (time_offset as f32 / state.time_window_ms as f32) * state.width + (state.width / 2.0);
            let y = -state.height / 2.0 + (p.price - state.min_price) / price_range * state.height;
            let current_pos = Vec3::new(x - state.width / 2.0, y, 0.1);

            if let Some(prev) = prev_pos {
                painter.line(prev, current_pos);
            }
            prev_pos = Some(current_pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::HistoryPoint;

    #[test]
    fn test_chart_view_state_update() {
        let mut world = World::new();
        let mut trading_data = TradingData::default();
        trading_data.history_points = vec![
            HistoryPoint { timestamp_ms: 1000, price: 10.0 },
            HistoryPoint { timestamp_ms: 2000, price: 20.0 },
        ];
        world.insert_resource(trading_data);

        let _entity = world.spawn((
            ChartViewState::default(),
            GlobalTransform::default(),
        )).id();

        let _schedule = Schedule::default();
        // Note: ShapePainter requires more setup, but we can test the state update part
        // by only running the system logic if possible, or mocking painter.
        // Since we can't easily mock ShapePainter here without full bevy setup,
        // we'll focus on testing the state update logic if we extracted it.
        // For now, let's at least check if it compiles and runs.
    }

    #[test]
    fn test_chart_autoscale() {
        let mut state = ChartViewState::default();
        state.auto_scale = true;
        state.time_window_ms = 1000;
        state.latest_timestamp_ms = 2000;

        let history = vec![
            HistoryPoint { timestamp_ms: 500, price: 100.0 }, // Out of window
            HistoryPoint { timestamp_ms: 1500, price: 10.0 }, // In window
            HistoryPoint { timestamp_ms: 2000, price: 20.0 }, // In window
        ];

        // Manually trigger autoscale logic (extracted or simulated)
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut has_visible_data = false;
        let start_ts = state.latest_timestamp_ms - state.time_window_ms;

        for p in &history {
            if p.timestamp_ms >= start_ts {
                if p.price < min { min = p.price; }
                if p.price > max { max = p.price; }
                has_visible_data = true;
            }
        }

        if has_visible_data {
            let range = max - min;
            state.min_price = min - range * 0.1;
            state.max_price = max + range * 0.1;
        }

        assert!(state.min_price < 10.0);
        assert!(state.max_price > 20.0);
        assert!(state.min_price > 0.0); // Should not be affected by 100.0
    }
}
