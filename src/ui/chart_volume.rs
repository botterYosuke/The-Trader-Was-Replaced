//! Volume サブペイン描画 (Phase 7.3 Phase E)。
//!
//! flowsurface (`.claude/skills/flowsurface/src/src/chart/kline.rs` の volume bar) を Bevy に
//! 翻訳する。volume area は Phase A で `ChartViewState` が「draw 領域下部 20%」として予約済み
//! (`volume_area_height()` / `main_area_y_bottom()`) なので Phase E は純 additive: main draw /
//! axes / interaction / crosshair の座標式を一切触らない。
//!
//! ⚠️ ShapePainter は immediate-mode なので `Changed` で gate しない (Caveat #11):
//! filter で early-out すると変化が無いフレームで bar が消える。`InstrumentTradingDataMap` /
//! `ChartViewState` は read-only に取り毎フレーム純 draw する。描画スキップは per-entity の
//! `continue` で行う。

use crate::trading::{InstrumentTradingDataMap, OhlcPoint};
use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::components::ChartInstrument;
use crate::ui::theme::Theme;
use bevy::prelude::*;
use bevy_vector_shapes::prelude::*;

/// volume bar の z (背景 rect の上、candle body +0.2 / crosshair +0.5 より下)。
const VOLUME_BAR_Z: f32 = 0.15;
/// volume bar は candle と同色を半透明にして描く。
const VOLUME_BAR_ALPHA: f32 = 0.6;

/// 可視 candle 中の最大 volume。`None` の candle は除外。0 件 / 全 `None` なら 0.0。
pub fn max_visible_volume(visible: &[OhlcPoint]) -> f32 {
    visible
        .iter()
        .filter_map(|c| c.volume)
        .fold(0.0_f32, f32::max)
}

/// volume bar の高さ (px)。`max_volume <= 0` のとき 0 (描画スキップ判定に使う)。
/// 比率は `[0, 1]` にクランプして volume area の高さを超えないようにする。
pub fn volume_bar_height(vol: f32, max_volume: f32, area_height: f32) -> f32 {
    if max_volume <= 0.0 {
        return 0.0;
    }
    (vol / max_volume).clamp(0.0, 1.0) * area_height
}

/// volume を gutter 幅 (50px) に収まる短い文字列へ整形 (K/M/B 略記)。crosshair badge が使う。
pub fn format_volume(vol: f32) -> String {
    let v = vol.abs();
    if v >= 1_000_000_000.0 {
        format!("{:.1}B", vol / 1_000_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.1}M", vol / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", vol / 1_000.0)
    } else {
        format!("{:.0}", vol)
    }
}

/// volume bar を毎フレーム描く。filter 無しの全 chart entity ループ。
///
/// bar は draw 領域の下端 (`-bounds.y / 2`) から最大 `volume_area_height()` まで上方向に伸びる
/// (= Phase A で予約した volume area にちょうど収まる)。bar 幅は candle body と同じ
/// (`body_half_width() * 2`) なので zoom に追随する。
pub fn volume_render_system(
    mut painter: ShapePainter,
    map: Res<InstrumentTradingDataMap>,
    chart_q: Query<(&GlobalTransform, &ChartInstrument, &ChartViewState)>,
    theme: Res<Theme>,
) {
    for (gt, instrument, state) in &chart_q {
        let Some(data) = map.map.get(&instrument.instrument_id) else {
            continue;
        };
        // autoscale と同一スライス (Phase A の helper を再利用)。
        let visible = state.visible_candle_slice(&data.ohlc_points);
        let max_volume = max_visible_volume(visible);
        if max_volume <= 0.0 {
            continue; // volume データ無し (全 None or 空) の銘柄は volume area 空のまま。
        }

        let origin = gt.translation();
        let bar_width = state.body_half_width() * 2.0;
        let bar_bottom_y = -state.bounds.y / 2.0;
        let area_height = state.volume_area_height();
        let bullish = theme.status.long.with_alpha(VOLUME_BAR_ALPHA);
        let bearish = theme.status.short.with_alpha(VOLUME_BAR_ALPHA);

        for candle in visible {
            let Some(vol) = candle.volume else {
                continue; // None の candle は skip (Some(0.0) とは区別する)。
            };
            let height = volume_bar_height(vol, max_volume, area_height);
            if height <= 0.0 {
                continue;
            }
            let x = state.interval_to_x(candle.open_time_ms);
            painter.color = if candle.close >= candle.open {
                bullish
            } else {
                bearish
            };
            painter.set_translation(Vec3::new(
                origin.x + x,
                origin.y + bar_bottom_y + height / 2.0,
                origin.z + VOLUME_BAR_Z,
            ));
            painter.rect(Vec2::new(bar_width, height));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::OhlcPoint;

    fn ohlc_vol(open_time_ms: i64, vol: Option<f32>) -> OhlcPoint {
        OhlcPoint {
            timestamp_ms: open_time_ms,
            open_time_ms,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: vol,
        }
    }

    #[test]
    fn max_visible_volume_skips_none() {
        let candles = [
            ohlc_vol(0, Some(10.0)),
            ohlc_vol(60_000, None),
            ohlc_vol(120_000, Some(42.0)),
            ohlc_vol(180_000, Some(7.0)),
        ];
        assert_eq!(max_visible_volume(&candles), 42.0);
    }

    #[test]
    fn max_visible_volume_zero_when_all_none_or_empty() {
        assert_eq!(max_visible_volume(&[]), 0.0);
        let all_none = [ohlc_vol(0, None), ohlc_vol(60_000, None)];
        assert_eq!(max_visible_volume(&all_none), 0.0);
    }

    #[test]
    fn volume_bar_height_proportional_to_max() {
        let area = 40.0;
        // max volume → 高さ = area。
        assert!((volume_bar_height(100.0, 100.0, area) - area).abs() < 1e-3);
        // 半分 → area の半分。
        assert!((volume_bar_height(50.0, 100.0, area) - area / 2.0).abs() < 1e-3);
        // 0 → 0。
        assert_eq!(volume_bar_height(0.0, 100.0, area), 0.0);
    }

    #[test]
    fn volume_bar_height_zero_when_no_max() {
        // max_volume <= 0 のとき (data 無し) は常に 0 (描画スキップ)。
        assert_eq!(volume_bar_height(10.0, 0.0, 40.0), 0.0);
        assert_eq!(volume_bar_height(10.0, -1.0, 40.0), 0.0);
    }

    #[test]
    fn volume_bar_height_clamps_above_area() {
        // vol > max_volume (異常値) でも area を超えない。
        let area = 40.0;
        assert!((volume_bar_height(200.0, 100.0, area) - area).abs() < 1e-3);
    }

    #[test]
    fn format_volume_abbreviates() {
        assert_eq!(format_volume(950.0), "950");
        assert_eq!(format_volume(1_500.0), "1.5K");
        assert_eq!(format_volume(2_300_000.0), "2.3M");
        assert_eq!(format_volume(4_100_000_000.0), "4.1B");
    }
}
