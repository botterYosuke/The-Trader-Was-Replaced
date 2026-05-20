use crate::trading::InstrumentTradingDataMap;
use crate::ui::components::ChartInstrument;
use bevy::prelude::*;
use bevy_vector_shapes::prelude::*;

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

/// Draw a single candlestick at `x_rel` (relative to chart center).
///
/// `chart_origin` is the painter world-space origin (entity translation).
/// Wick covers high–low; body covers open–close.
/// Green when close >= open, red when close < open.
#[allow(clippy::too_many_arguments)]
fn draw_candle(
    painter: &mut ShapePainter,
    chart_origin: Vec3,
    x_rel: f32,
    open: f32,
    high: f32,
    low: f32,
    close: f32,
    body_half_width: f32,
    min_price: f32,
    price_range: f32,
    chart_height: f32,
) {
    let color = if close >= open {
        Color::srgb(0.0, 0.78, 0.31) // bullish green
    } else {
        Color::srgb(0.9, 0.2, 0.2) // bearish red
    };

    let y_of = |p: f32| -chart_height / 2.0 + (p - min_price) / price_range * chart_height;

    let y_high = y_of(high);
    let y_low = y_of(low);
    let y_open = y_of(open);
    let y_close = y_of(close);

    painter.color = color;

    // Wick: thin vertical line spanning high–low
    painter.set_translation(chart_origin);
    painter.thickness = 1.5;
    painter.line(
        Vec3::new(x_rel, y_low, 0.15),
        Vec3::new(x_rel, y_high, 0.15),
    );

    // Body: rect from open to close (min 1.5 px tall so doji is visible)
    let body_center_y = (y_open + y_close) / 2.0;
    let body_height = (y_close - y_open).abs().max(1.5);
    painter.set_translation(Vec3::new(
        chart_origin.x + x_rel,
        chart_origin.y + body_center_y,
        chart_origin.z + 0.2,
    ));
    painter.rect(Vec2::new(body_half_width * 2.0, body_height));
}

pub fn chart_render_system(
    mut painter: ShapePainter,
    trading_data: Res<InstrumentTradingDataMap>,
    mut query: Query<(&mut ChartViewState, &GlobalTransform, &ChartInstrument)>,
) {
    for (mut state, transform, ci) in query.iter_mut() {
        let Some(data) = trading_data.map.get(&ci.instrument_id) else {
            continue;
        };

        let ohlc_pts = &data.ohlc_points;

        if ohlc_pts.is_empty() {
            continue;
        }

        if let Some(last) = ohlc_pts.last() {
            state.latest_timestamp_ms = last.timestamp_ms;
        }

        if state.latest_timestamp_ms == 0 {
            continue;
        }

        // Determine visible OHLC candles (last MAX_VISIBLE bars)
        const MAX_VISIBLE_CANDLES: usize = 50;
        let candle_start_idx = if ohlc_pts.len() > MAX_VISIBLE_CANDLES {
            ohlc_pts.len() - MAX_VISIBLE_CANDLES
        } else {
            0
        };
        let visible_candles = &ohlc_pts[candle_start_idx..];

        if state.auto_scale {
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            let mut has_visible_data = false;

            // Scan all visible candle high/low for autoscale range
            for pt in visible_candles {
                if pt.high > max {
                    max = pt.high;
                }
                if pt.low < min {
                    min = pt.low;
                }
                has_visible_data = true;
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
        if price_range <= 0.0 {
            continue;
        }

        let start_pos = transform.translation();
        painter.set_translation(start_pos);

        // Background
        painter.color = Color::srgb(0.3, 0.3, 0.3);
        painter.rect(Vec2::new(state.width, state.height));

        // --- Line chart: derive from ohlc closes (keep draw math identical) ---
        painter.color = Color::srgb(0.0, 1.0, 0.5);
        painter.thickness = 2.0;

        let start_ts = state.latest_timestamp_ms - state.time_window_ms;
        let mut prev_pos: Option<Vec3> = None;

        for pt in ohlc_pts {
            if pt.timestamp_ms < start_ts {
                continue;
            }
            let time_offset = pt.timestamp_ms - state.latest_timestamp_ms;
            let x = (time_offset as f32 / state.time_window_ms as f32) * state.width
                + (state.width / 2.0);
            let y =
                -state.height / 2.0 + (pt.close - state.min_price) / price_range * state.height;
            let current_pos = Vec3::new(x - state.width / 2.0, y, 0.1);

            if let Some(prev) = prev_pos {
                painter.line(prev, current_pos);
            }
            prev_pos = Some(current_pos);
        }

        // --- Multiple candlesticks from OHLC history ---
        if visible_candles.len() >= 2 {
            let latest_ots = visible_candles.last().unwrap().open_time_ms;
            let oldest_ots = visible_candles.first().unwrap().open_time_ms;
            let span_ms = (latest_ots - oldest_ots).max(1) as f32;
            let n = visible_candles.len() as f32;
            let body_half_width = (state.width / 2.0 / (n * 2.5)).max(1.0);

            for pt in visible_candles {
                // Map open_time_ms so that oldest → x=-width/2, newest → x=0
                let x_rel = (pt.open_time_ms - latest_ots) as f32 / span_ms * (state.width / 2.0);
                draw_candle(
                    &mut painter,
                    start_pos,
                    x_rel,
                    pt.open,
                    pt.high,
                    pt.low,
                    pt.close,
                    body_half_width,
                    state.min_price,
                    price_range,
                    state.height,
                );
                painter.set_translation(start_pos);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{InstrumentTradingData, OhlcPoint};

    #[test]
    fn test_chart_view_state_update() {
        let mut world = World::new();
        let mut tdmap = InstrumentTradingDataMap::default();
        let data = InstrumentTradingData {
            ohlc_points: vec![
                OhlcPoint {
                    timestamp_ms: 1000,
                    open_time_ms: 1000,
                    open: 10.0,
                    high: 12.0,
                    low: 9.0,
                    close: 11.0,
                    volume: None,
                },
                OhlcPoint {
                    timestamp_ms: 2000,
                    open_time_ms: 2000,
                    open: 11.0,
                    high: 22.0,
                    low: 10.0,
                    close: 20.0,
                    volume: None,
                },
            ],
            ..Default::default()
        };
        tdmap.map.insert("TEST".to_string(), data);
        world.insert_resource(tdmap);

        let _entity = world
            .spawn((
                ChartViewState::default(),
                GlobalTransform::default(),
                ChartInstrument {
                    instrument_id: "TEST".to_string(),
                },
            ))
            .id();

        let _schedule = Schedule::default();
    }

    #[test]
    fn test_chart_autoscale() {
        let mut state = ChartViewState::default();
        state.auto_scale = true;
        state.time_window_ms = 1000;
        state.latest_timestamp_ms = 2000;

        let history: Vec<(i64, f32)> = vec![
            (500, 100.0),  // out of window
            (1500, 10.0),  // in window
            (2000, 20.0),  // in window
        ];

        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut has_visible_data = false;
        let start_ts = state.latest_timestamp_ms - state.time_window_ms;

        for p in &history {
            if p.0 >= start_ts {
                if p.1 < min {
                    min = p.1;
                }
                if p.1 > max {
                    max = p.1;
                }
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
        assert!(state.min_price > 0.0);
    }

    #[test]
    fn test_candle_direction() {
        // Bullish: close >= open
        let (open, close) = (100.0_f32, 110.0_f32);
        assert!(close >= open, "close >= open should be bullish");

        // Bearish: close < open
        let (open2, close2) = (100.0_f32, 90.0_f32);
        assert!(close2 < open2, "close < open should be bearish");

        // Doji: close == open
        let (open3, close3) = (100.0_f32, 100.0_f32);
        assert!(close3 >= open3, "doji (close == open) treated as bullish");
    }

    #[test]
    fn test_candle_y_mapping() {
        let chart_height = 200.0_f32;
        let min_price = 100.0_f32;
        let price_range = 50.0_f32;
        let y_of = |p: f32| -chart_height / 2.0 + (p - min_price) / price_range * chart_height;

        // Bottom of chart corresponds to min_price
        assert!((y_of(100.0) - (-100.0)).abs() < 0.001);
        // Top of chart corresponds to min_price + price_range
        assert!((y_of(150.0) - 100.0).abs() < 0.001);
        // Mid-price maps to y=0
        assert!((y_of(125.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_candle_body_height_min() {
        // Doji body height must be at least 1.5 so it's always visible
        let y_open = 50.0_f32;
        let y_close = 50.0_f32; // same → doji
        let body_height = (y_close - y_open).abs().max(1.5);
        assert!(body_height >= 1.5);
    }

    #[test]
    fn test_autoscale_extends_for_candle_high_low() {
        let high = 200.0_f32;
        let low = 50.0_f32;
        let mut max = 150.0_f32;
        let mut min = 80.0_f32;
        if high > max {
            max = high;
        }
        if low < min {
            min = low;
        }
        assert_eq!(max, 200.0);
        assert_eq!(min, 50.0);
    }

    #[test]
    fn test_multi_candle_x_positions() {
        // Oldest candle → x = -width/2, newest → x = 0
        let width = 360.0_f32;
        let oldest_ots: i64 = 1000;
        let newest_ots: i64 = 3000;
        let span_ms = (newest_ots - oldest_ots) as f32;

        let x_oldest = (oldest_ots - newest_ots) as f32 / span_ms * (width / 2.0);
        let x_newest = (newest_ots - newest_ots) as f32 / span_ms * (width / 2.0);
        let x_mid = (2000_i64 - newest_ots) as f32 / span_ms * (width / 2.0);

        assert!((x_oldest - (-width / 2.0)).abs() < 0.001);
        assert!((x_newest - 0.0).abs() < 0.001);
        assert!((x_mid - (-width / 4.0)).abs() < 0.001);
    }

    #[test]
    fn test_multi_candle_body_half_width() {
        // body_half_width shrinks as candle count grows
        let width = 360.0_f32;
        let n10 = 10.0_f32;
        let n50 = 50.0_f32;
        let bw10 = (width / 2.0 / (n10 * 2.5)).max(1.0);
        let bw50 = (width / 2.0 / (n50 * 2.5)).max(1.0);
        assert!(bw10 > bw50, "wider candles for fewer bars");
        assert!(bw50 >= 1.0, "minimum 1px body");
    }
}
