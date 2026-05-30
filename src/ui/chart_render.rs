//! main chart の毎フレーム描画 (背景 + close ライン + ローソク足)。
//!
//! ⚠️ ShapePainter は immediate-mode なので `Changed` で gate しない (Caveat #11/プラン
//! 「immediate-mode 罠」): filter で early-out すると変化が無いフレームで candle が消える。
//! `ChartViewState` は **read-only** に取り、autoscale は `chart_viewstate.rs` 側に任せる
//! (この system は純 draw)。

use crate::trading::{InstrumentTradingDataMap, OhlcPoint};
use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::components::ChartInstrument;
use crate::ui::theme::Theme;
use bevy::prelude::*;
use bevy_vector_shapes::prelude::*;

/// 1 本のローソク足を描く。`x` は chart-local x (= `interval_to_x` の結果)。
fn draw_candle(
    painter: &mut ShapePainter,
    chart_origin: Vec3,
    x: f32,
    pt: &OhlcPoint,
    body_half_width: f32,
    state: &ChartViewState,
    bullish_color: Color,
    bearish_color: Color,
) {
    let color = if pt.close >= pt.open {
        bullish_color
    } else {
        bearish_color
    };

    let y_high = state.price_to_y(pt.high);
    let y_low = state.price_to_y(pt.low);
    let y_open = state.price_to_y(pt.open);
    let y_close = state.price_to_y(pt.close);

    painter.color = color;

    // Wick: high–low を結ぶ細い縦線。
    painter.set_translation(chart_origin);
    painter.thickness = 1.5;
    painter.line(Vec3::new(x, y_low, 0.15), Vec3::new(x, y_high, 0.15));

    // Body: open–close の矩形 (doji が見えるよう最低 1.5px)。
    let body_center_y = (y_open + y_close) / 2.0;
    let body_height = (y_close - y_open).abs().max(1.5);
    painter.set_translation(Vec3::new(
        chart_origin.x + x,
        chart_origin.y + body_center_y,
        chart_origin.z + 0.2,
    ));
    painter.rect(Vec2::new(body_half_width * 2.0, body_height));
}

/// main chart を毎フレーム描く。filter 無しの全 chart entity ループ。
pub fn chart_main_render_system(
    mut painter: ShapePainter,
    trading_data: Res<InstrumentTradingDataMap>,
    query: Query<(&ChartViewState, &GlobalTransform, &ChartInstrument)>,
    theme: Res<Theme>,
) {
    for (state, transform, ci) in &query {
        let Some(data) = trading_data.map.get(&ci.instrument_id) else {
            continue;
        };
        let origin = transform.translation();

        painter.set_translation(origin);
        painter.color = theme.colors.background;
        painter.rect(state.bounds);

        let ohlc = &data.ohlc_points;
        if ohlc.is_empty() {
            continue;
        }
        let visible = state.visible_candle_slice(ohlc);
        if visible.is_empty() {
            continue;
        }

        // close ライン (z=0.1) と candle (wick z=0.15 / body z=0.2) を 1 パスで描く。
        // x は candle あたり 1 回だけ算出 (z 差で描画順は気にしなくて良い)。
        let body_half_width = state.body_half_width();
        let line_color = theme.status.success;
        let bullish_color = theme.status.long;
        let bearish_color = theme.status.short;
        let mut prev: Option<Vec3> = None;
        for pt in visible {
            let x = state.interval_to_x(pt.open_time_ms);
            let current = Vec3::new(x, state.price_to_y(pt.close), 0.1);
            if let Some(p) = prev {
                painter.set_translation(origin);
                painter.color = line_color;
                painter.thickness = 2.0;
                painter.line(p, current);
            }
            prev = Some(current);

            draw_candle(&mut painter, origin, x, pt, body_half_width, state, bullish_color, bearish_color);
            painter.set_translation(origin);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::chart_viewstate::ChartViewState;

    /// body_half_width は cell_width に比例し最低 0.5px。
    #[test]
    fn body_half_width_floor() {
        let mut state = ChartViewState::default();
        state.cell_width = 0.1;
        state.scaling = 1.0;
        let bw = state.body_half_width();
        assert!(bw >= 0.5);

        state.cell_width = 20.0;
        let bw2 = state.body_half_width();
        assert!(bw2 > bw, "wider cells → wider body");
    }
}
