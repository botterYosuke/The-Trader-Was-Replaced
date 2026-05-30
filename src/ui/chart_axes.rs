//! 価格軸 (Y) / 時間軸 (X) のラベル生成 (Phase 7.3 Phase B)。
//!
//! flowsurface (`.claude/skills/flowsurface/src/src/chart/scale/`) の
//! `linear.rs::calc_optimal_ticks` / `timeseries.rs::calc_time_step` を Bevy に翻訳する。
//!
//! ラベルは gutter (chart 右の price gutter / chart 下の time gutter) の子 Text2d entity として
//! `Changed<ChartViewState>` 駆動で despawn+respawn する (Cache の y_labels/x_labels 層に相当)。
//! ⚠️ ShapePainter ではなく retained-mode の Text2d なので `Changed` gate して良い (Caveat #11)。
//! ⚠️ `commands.entity(gutter).despawn()` は子孫を despawn しない (Caveat #26) ので、ラベルは
//! `target_chart` を見て個別 despawn する。

use crate::ui::chart_viewstate::{ChartViewState, PRICE_GUTTER_WIDTH};
use crate::ui::components::ChartInstrument;
use crate::ui::theme::Theme;
use bevy::prelude::*;
use bevy::sprite::Anchor;

/// ラベルのフォントサイズ (px)。gutter 幅 (50) / 高さ (24) に収まる小さめの値。
const TEXT_SIZE: f32 = 11.0;
/// axis label の z (Caveat #16: crosshair badge は +0.6、cross line は +0.5)。
const LABEL_Z: f32 = 0.3;
/// price label の左パディング (gutter 左端からの余白)。
const PRICE_LABEL_PAD_X: f32 = 3.0;
/// price label 1 個あたりの最小縦ピッチ (`TEXT_SIZE` 倍)。flowsurface `generate_labels` と同じ係数 3。
const PRICE_LABEL_PITCH: f32 = 3.0;
/// time label 1 個あたりの最小横ピッチ (`TEXT_SIZE` 倍)。"HH:MM" 幅 + 余白の経験値。
const TIME_LABEL_PITCH: f32 = 4.5;
/// 隣接 time label の最小ピクセル間隔。flowsurface の step 選択は want を超える本数を返す
/// ことがある (break 条件が「want 本以上で最も細かい step」なので overshoot する) ため、
/// 描画段で間引いて "HH:MM" (≈33px) が重ならないようにする。
const MIN_TIME_LABEL_SPACING: f32 = 44.0;
/// 暴走防止 (flowsurface の MAX_ITERATIONS と同じ役割)。通常の labels_can_fit (~10-30) より十分大きい。
const MAX_LABELS: usize = 64;

// ─── Component (gutter とラベル) ───

/// 価格ラベルを並べる gutter (chart の右、`PRICE_GUTTER_WIDTH` 幅)。chart entity の子。
#[derive(Component)]
pub struct PriceGutter;
/// 時刻ラベルを並べる gutter (chart の下、`TIME_GUTTER_HEIGHT` 高)。chart entity の子。
#[derive(Component)]
pub struct TimeGutter;
/// chart entity 側に持たせる price gutter への参照。
#[derive(Component)]
pub struct PriceGutterRef(pub Entity);
/// chart entity 側に持たせる time gutter への参照。
#[derive(Component)]
pub struct TimeGutterRef(pub Entity);
/// price gutter 内の 1 ラベル。どの chart のものかで despawn 対象を絞る。
#[derive(Component)]
pub struct PriceLabel {
    pub target_chart: Entity,
}
/// time gutter 内の 1 ラベル。
#[derive(Component)]
pub struct TimeLabel {
    pub target_chart: Entity,
}

// ─── tick 計算 (純関数、flowsurface 翻訳) ───

/// flowsurface `scale/linear.rs::calc_optimal_ticks` の翻訳。
/// `(step, rounded_highest)` を返す。`step` は「人間に優しい」価格刻み。
pub fn calc_optimal_price_ticks(highest: f32, lowest: f32, labels_can_fit: i32) -> (f32, f32) {
    let range = (highest - lowest).abs().max(f32::EPSILON);
    let labels = labels_can_fit.max(1) as f32;

    let base = 10.0_f32.powf(range.log10().floor());

    let step = match range / base {
        r if r <= labels * 0.1 => 0.1 * base,
        r if r <= labels * 0.2 => 0.2 * base,
        r if r <= labels * 0.5 => 0.5 * base,
        r if r <= labels => base,
        r if r <= labels * 2.0 => 2.0 * base,
        _ => (range / labels).min(5.0 * base),
    };

    let rounded_highest = (highest / step).ceil() * step;
    (step, rounded_highest)
}

// flowsurface `scale/timeseries.rs` の step テーブル (ms)。Basis::Time(timeframe_ms) ごとに選ぶ。
const M1_TIME_STEPS: [u64; 9] = [
    720 * 60_000, // 12h
    180 * 60_000, // 3h
    60 * 60_000,  // 1h
    30 * 60_000,  // 30m
    15 * 60_000,  // 15m
    10 * 60_000,  // 10m
    5 * 60_000,   // 5m
    2 * 60_000,   // 2m
    60_000,       // 1m
];
const M3_TIME_STEPS: [u64; 9] = [
    1440 * 60_000,
    720 * 60_000,
    360 * 60_000,
    120 * 60_000,
    60 * 60_000,
    30 * 60_000,
    15 * 60_000,
    9 * 60_000,
    3 * 60_000,
];
const M5_TIME_STEPS: [u64; 9] = [
    1440 * 60_000,
    720 * 60_000,
    480 * 60_000,
    240 * 60_000,
    120 * 60_000,
    60 * 60_000,
    30 * 60_000,
    15 * 60_000,
    5 * 60_000,
];
const HOURLY_TIME_STEPS: [u64; 8] = [
    5760 * 60_000,
    2880 * 60_000,
    1440 * 60_000,
    720 * 60_000,
    480 * 60_000,
    240 * 60_000,
    120 * 60_000,
    60 * 60_000,
];
const MS_TIME_STEPS: [u64; 10] = [
    120_000, 60_000, 30_000, 10_000, 5_000, 2_000, 1_000, 500, 200, 100,
];

/// timeframe (ms) に対応する step テーブルを選ぶ (flowsurface `calc_time_step` の match)。
///
/// flowsurface 同様、1/3/5/15/30 分以外の 1..=30 分 (2,4,6-14,16-29) と 31 分以上は
/// すべて `HOURLY_TIME_STEPS` にフォールバックする (timeseries.rs:84-92 の内側 `_` 相当)。
fn time_steps_for(timeframe_ms: u64) -> &'static [u64] {
    let tf_min = timeframe_ms / 60_000;
    match tf_min {
        0 => &MS_TIME_STEPS,
        1 => &M1_TIME_STEPS,
        3 => &M3_TIME_STEPS,
        5 => &M5_TIME_STEPS,
        15 => &M5_TIME_STEPS[..7],
        30 => &M5_TIME_STEPS[..6],
        _ => &HOURLY_TIME_STEPS,
    }
}

/// flowsurface `scale/timeseries.rs::calc_time_step` の翻訳。
/// `(step_ms, rounded_earliest_ms)` を返す。step が 0 を返すことはない (テーブルは全て > 0)。
pub fn calc_optimal_time_step(
    earliest_ms: i64,
    latest_ms: i64,
    labels_can_fit: i32,
    timeframe_ms: u64,
) -> (u64, i64) {
    let time_steps = time_steps_for(timeframe_ms);
    let duration = (latest_ms - earliest_ms).max(0) as u64;
    let want = labels_can_fit.max(1) as u64;

    let mut selected_step = time_steps[0];
    for &step in time_steps {
        if step > 0 && duration / step >= want {
            selected_step = step;
            break;
        }
        if step <= duration {
            selected_step = step;
        }
    }

    let step_i = selected_step as i64;
    let rounded_earliest = if step_i > 0 {
        (earliest_ms.div_euclid(step_i)) * step_i
    } else {
        earliest_ms
    };
    (selected_step, rounded_earliest)
}

// ─── システム (Changed<ChartViewState> 駆動) ───

/// 価格軸ラベルを再生成する。`Changed<ChartViewState>` のフレームのみ走る。
pub fn price_axis_labels_system(
    mut commands: Commands,
    chart_q: Query<
        (Entity, &ChartViewState, &PriceGutterRef),
        (With<ChartInstrument>, Changed<ChartViewState>),
    >,
    existing: Query<(Entity, &PriceLabel)>,
    // gutter が生存している chart のみ処理する。chart panel は prune→sync で spawn 直後に
    // despawn されることがあり (universe 空など)、despawn 済 gutter への set_parent は panic する。
    live_gutter: Query<(), With<PriceGutter>>,
    theme: Res<Theme>,
) {
    for (chart_entity, state, gutter_ref) in &chart_q {
        if !live_gutter.contains(gutter_ref.0) {
            continue;
        }
        // 全ラベルを走査して自 chart 分だけ despawn する (C×L だが C は変化した chart のみ＝
        // 通常 1-2、L は数十なので無視できる)。
        for (label_e, label) in &existing {
            if label.target_chart == chart_entity {
                commands.entity(label_e).despawn();
            }
        }

        // ⚠️ main area (上 80%) の価格域だけにラベルを引く。フル bounds だと volume sub-pane
        //    (下 20%) の y 行に価格目盛りが出てしまい、volume bar と無関係な価格が並ぶ (Phase E)。
        let (low, high) = state.main_area_price_range();
        if !low.is_finite() || !high.is_finite() || (high - low).abs() < f32::EPSILON {
            continue;
        }
        let labels_can_fit =
            (state.main_area_height() / (TEXT_SIZE * PRICE_LABEL_PITCH)).max(1.0) as i32;
        let (step, rounded_max) = calc_optimal_price_ticks(high, low, labels_can_fit);
        if step <= 0.0 || !step.is_finite() {
            continue;
        }

        // gutter local: 原点は chart-local (180, 0)。price_to_y は chart-local y を返し、
        // gutter は chart-local y=0 に置いているので gutter-local y == price_to_y(value)。
        let label_x = -PRICE_GUTTER_WIDTH / 2.0 + PRICE_LABEL_PAD_X;

        let mut value = rounded_max;
        while value > high {
            value -= step;
        }
        let mut count = 0;
        while value >= low && count < MAX_LABELS {
            let y = state.price_to_y(value);
            commands
                .spawn((
                    Text2d::new(format!("{:.*}", state.decimals, value)),
                    TextFont {
                        font_size: TEXT_SIZE,
                        ..default()
                    },
                    TextColor(theme.colors.text_muted),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(label_x, y, LABEL_Z),
                    PriceLabel {
                        target_chart: chart_entity,
                    },
                ))
                .insert(ChildOf(gutter_ref.0));
            value -= step;
            count += 1;
        }
    }
}

/// 時間軸ラベルを再生成する。`Changed<ChartViewState>` のフレームのみ走る。UTC 固定 (Caveat #18)。
pub fn time_axis_labels_system(
    mut commands: Commands,
    chart_q: Query<
        (Entity, &ChartViewState, &TimeGutterRef),
        (With<ChartInstrument>, Changed<ChartViewState>),
    >,
    existing: Query<(Entity, &TimeLabel)>,
    // 価格軸と同様、despawn 済 gutter への set_parent panic を防ぐ。
    live_gutter: Query<(), With<TimeGutter>>,
    theme: Res<Theme>,
) {
    for (chart_entity, state, gutter_ref) in &chart_q {
        if !live_gutter.contains(gutter_ref.0) {
            continue;
        }
        for (label_e, label) in &existing {
            if label.target_chart == chart_entity {
                commands.entity(label_e).despawn();
            }
        }

        let (earliest, latest) = state.visible_time_range();
        if latest <= earliest {
            continue;
        }
        let timeframe_ms = state.timeframe_ms();
        let labels_can_fit = (state.bounds.x / (TEXT_SIZE * TIME_LABEL_PITCH)).max(1.0) as i32;
        let (step, rounded_earliest) =
            calc_optimal_time_step(earliest, latest, labels_can_fit, timeframe_ms);
        if step == 0 {
            continue;
        }
        let step_i = step as i64;

        // chart 描画域の x 範囲 (この外に来るラベルは gutter からはみ出すので描かない)。
        let half_x = state.bounds.x / 2.0;

        let mut t = rounded_earliest;
        while t < earliest {
            t += step_i;
        }
        // x 昇順に走査し、直前ラベルから MIN_TIME_LABEL_SPACING 以上離れたものだけ描く (重なり防止)。
        let mut count = 0;
        let mut last_x: Option<f32> = None;
        while t <= latest && count < MAX_LABELS {
            let x = state.interval_to_x(t);
            let far_enough = last_x.map_or(true, |lx| x - lx >= MIN_TIME_LABEL_SPACING);
            if x >= -half_x && x <= half_x && far_enough {
                if let Some(text) = format_time_label(t) {
                    commands
                        .spawn((
                            Text2d::new(text),
                            TextFont {
                                font_size: TEXT_SIZE,
                                ..default()
                            },
                            TextColor(theme.colors.text_muted),
                            Anchor::CENTER,
                            // time gutter は chart-local (0, -y) に置いているので gutter-local x == interval_to_x。
                            Transform::from_xyz(x, 0.0, LABEL_Z),
                            TimeLabel {
                                target_chart: chart_entity,
                            },
                        ))
                        .insert(ChildOf(gutter_ref.0));
                    last_x = Some(x);
                }
            }
            t += step_i;
            count += 1;
        }
    }
}

/// ms (UTC) を "HH:MM" にフォーマットする。日付跨ぎは Phase E polish の余地 (今は時刻のみ)。
/// Phase D crosshair の time badge も同じフォーマットを使うため `pub(crate)`。
pub(crate) fn format_time_label(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|dt| dt.format("%H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_ticks_smoke_110_100_10() {
        // flowsurface 同等の (step=1.0, max=110.0)。
        let (step, max) = calc_optimal_price_ticks(110.0, 100.0, 10);
        assert!((step - 1.0).abs() < 1e-4, "step={}", step);
        assert!((max - 110.0).abs() < 1e-4, "max={}", max);
    }

    #[test]
    fn price_ticks_fixed_cases() {
        // 各レンジ × labels_can_fit で step が range/labels を概ね満たし、max >= highest。
        let cases = [
            (0.05_f32, 0.01_f32, 3),
            (1.0, 0.0, 10),
            (250.0, 100.0, 10),
            (12345.0, 8000.0, 50),
            (105.0, 100.0, 5),
        ];
        for (hi, lo, fit) in cases {
            let (step, max) = calc_optimal_price_ticks(hi, lo, fit);
            assert!(step > 0.0, "step must be positive (hi={hi}, lo={lo})");
            assert!(
                max >= hi - step * 0.5,
                "rounded max {max} should cover {hi}"
            );
            // step で割り切れる位置に max が乗る。
            let k = (max / step).round();
            assert!(
                (k * step - max).abs() < step * 1e-3,
                "max {max} not on step {step}"
            );
            // labels_can_fit を極端に超えない (range/step <= fit*~10 程度)。
            let n = ((hi - lo).abs() / step).ceil();
            assert!(
                n <= (fit as f32) * 10.0 + 2.0,
                "too many ticks: {n} for fit={fit}"
            );
        }
    }

    #[test]
    fn time_step_m1_picks_minute_grid() {
        // 10 分窓・10 ラベル要求 → 1 分刻みが選ばれる (duration/step >= want で M1 末尾 60_000)。
        let earliest = 0;
        let latest = 10 * 60_000;
        let (step, rounded) = calc_optimal_time_step(earliest, latest, 10, 60_000);
        assert_eq!(step, 60_000, "10min window / 10 labels → 1min step");
        assert_eq!(rounded, 0, "rounded_earliest aligns to step");
    }

    #[test]
    fn time_step_m1_wide_window_coarsens() {
        // 24 時間窓 → 分刻みではラベルが多すぎるので時間単位の粗い step になる。
        let earliest = 0;
        let latest = 24 * 60 * 60_000;
        let (step, _rounded) = calc_optimal_time_step(earliest, latest, 6, 60_000);
        assert!(
            step >= 60 * 60_000,
            "wide window should pick >= 1h step, got {step}ms"
        );
    }

    #[test]
    fn time_step_rounds_down_to_grid() {
        // earliest がグリッド境界上に無い場合、rounded は直前の step 境界へ floor される。
        let step_ms = 60_000;
        let earliest = 5 * 60_000 + 12_345; // 5分 + 端数
        let (step, rounded) = calc_optimal_time_step(earliest, earliest + 9 * 60_000, 10, 60_000);
        assert_eq!(step, step_ms);
        assert_eq!(rounded, 5 * 60_000, "floor to minute boundary");
        assert!(rounded <= earliest);
    }

    #[test]
    fn time_step_unlisted_timeframe_uses_hourly_table() {
        // flowsurface 同様、2 分足など未列挙 timeframe は HOURLY テーブルを使う (M1 ではない)。
        // 48h 窓 / want=6 → HOURLY は 8h step、M1 だと 3h step になるので 8h が出れば HOURLY 確定。
        let earliest = 0;
        let latest = 48 * 60 * 60_000;
        let (step, _r) = calc_optimal_time_step(earliest, latest, 6, 2 * 60_000);
        assert_eq!(
            step,
            8 * 60 * 60_000,
            "2min timeframe must fall back to HOURLY table"
        );
    }

    #[test]
    fn time_step_never_zero() {
        // duration=0 でもテーブル先頭が選ばれ step>0。
        let (step, _r) = calc_optimal_time_step(1000, 1000, 5, 60_000);
        assert!(step > 0);
    }

    #[test]
    fn format_time_label_utc_hhmm() {
        // 1970-01-01 01:02:00 UTC = 3720_000 ms。
        assert_eq!(format_time_label(3_720_000).as_deref(), Some("01:02"));
    }

    /// Changed<ChartViewState> 駆動でラベルが gutter 子として生成され、target_chart が正しいこと。
    #[test]
    fn axis_systems_spawn_labels_as_gutter_children() {
        use crate::trading::{InstrumentTradingData, InstrumentTradingDataMap, OhlcPoint};
        use crate::ui::chart_viewstate::{
            ChartViewState, RequestAutoscale, chart_autoscale_apply_system, chart_data_tick_system,
            chart_interaction_tick_system,
        };

        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.add_message::<RequestAutoscale>();
        app.init_resource::<InstrumentTradingDataMap>();

        let mut data = InstrumentTradingData::default();
        for i in 0..20i64 {
            let base = 100.0 + i as f32;
            data.ohlc_points.push(OhlcPoint {
                timestamp_ms: i * 60_000,
                open_time_ms: i * 60_000,
                open: base,
                high: base + 2.0,
                low: base - 2.0,
                close: base + 1.0,
                volume: None,
            });
        }
        app.world_mut()
            .resource_mut::<InstrumentTradingDataMap>()
            .map
            .insert("T".to_string(), data);

        let price_gutter = app
            .world_mut()
            .spawn((PriceGutter, Transform::default()))
            .id();
        let time_gutter = app
            .world_mut()
            .spawn((TimeGutter, Transform::default()))
            .id();
        let chart = app
            .world_mut()
            .spawn((
                ChartViewState::default(),
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                PriceGutterRef(price_gutter),
                TimeGutterRef(time_gutter),
            ))
            .id();

        app.add_systems(
            Update,
            (
                chart_data_tick_system,
                chart_interaction_tick_system,
                chart_autoscale_apply_system,
                price_axis_labels_system,
                time_axis_labels_system,
            )
                .chain(),
        );

        app.update();
        app.update();

        let world = app.world_mut();
        let mut pq = world.query::<(&PriceLabel, &ChildOf)>();
        let price_labels: Vec<_> = pq.iter(world).collect();
        assert!(!price_labels.is_empty(), "price labels should be generated");
        for (label, parent) in &price_labels {
            assert_eq!(label.target_chart, chart);
            assert_eq!(
                parent.parent(),
                price_gutter,
                "price label must child of price gutter"
            );
        }

        let mut tq = world.query::<(&TimeLabel, &ChildOf, &Transform)>();
        let mut time_labels: Vec<_> = tq.iter(world).collect();
        assert!(!time_labels.is_empty(), "time labels should be generated");
        for (label, parent, _) in &time_labels {
            assert_eq!(label.target_chart, chart);
            assert_eq!(
                parent.parent(),
                time_gutter,
                "time label must child of time gutter"
            );
        }
        // 間引きにより隣接ラベルは MIN_TIME_LABEL_SPACING 以上離れている (重なり防止)。
        time_labels.sort_by(|a, b| a.2.translation.x.total_cmp(&b.2.translation.x));
        for pair in time_labels.windows(2) {
            let gap = pair[1].2.translation.x - pair[0].2.translation.x;
            assert!(
                gap >= MIN_TIME_LABEL_SPACING - 1e-3,
                "time labels too close: gap={gap} < {MIN_TIME_LABEL_SPACING}"
            );
        }
    }

    /// 価格ラベルは main area 内 (`y >= main_area_y_bottom`) にのみ生成され、volume sub-pane
    /// (下 20%) の y 行には出ないこと (Phase E 回帰防止)。
    #[test]
    fn price_labels_stay_within_main_area() {
        use crate::ui::chart_viewstate::ChartViewState;

        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();

        let price_gutter = app
            .world_mut()
            .spawn((PriceGutter, Transform::default()))
            .id();
        let time_gutter = app
            .world_mut()
            .spawn((TimeGutter, Transform::default()))
            .id();
        // autoscale 相当の実値を直接固定し、毎フレーム Changed を立てる。
        let mut state = ChartViewState::default();
        state.auto_scale = false;
        state.base_price_y = 105.0;
        // autoscale 出力相当: main_area_height * tick_size / range。range≈13 (98..111) を想定。
        state.cell_height = state.main_area_height() * state.tick_size / 13.0;
        state.latest_x = 9 * 60_000;
        let chart = app
            .world_mut()
            .spawn((
                state.clone(),
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                PriceGutterRef(price_gutter),
                TimeGutterRef(time_gutter),
            ))
            .id();

        app.add_systems(
            Update,
            (
                move |mut q: Query<&mut ChartViewState>| {
                    if let Ok(mut s) = q.get_mut(chart) {
                        s.set_changed();
                    }
                },
                price_axis_labels_system,
            )
                .chain(),
        );
        app.update();

        let bottom = state.main_area_y_bottom();
        let world = app.world_mut();
        let mut pq = world.query::<(&PriceLabel, &Transform)>();
        let labels: Vec<_> = pq.iter(world).collect();
        assert!(!labels.is_empty(), "price labels should be generated");
        for (_, tf) in &labels {
            assert!(
                tf.translation.y >= bottom - 1e-3,
                "price label at y={} leaked into volume area (bottom={bottom})",
                tf.translation.y
            );
        }
    }

    /// 連続した Changed フレームでラベルが累積せず置き換わること (despawn+respawn の検証)。
    #[test]
    fn axis_labels_replace_not_accumulate() {
        use crate::trading::{InstrumentTradingData, InstrumentTradingDataMap, OhlcPoint};
        use crate::ui::chart_viewstate::{ChartViewState, RequestAutoscale};

        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.add_message::<RequestAutoscale>();
        app.init_resource::<InstrumentTradingDataMap>();

        let mut data = InstrumentTradingData::default();
        for i in 0..10i64 {
            let base = 200.0 + i as f32;
            data.ohlc_points.push(OhlcPoint {
                timestamp_ms: i * 60_000,
                open_time_ms: i * 60_000,
                open: base,
                high: base + 1.0,
                low: base - 1.0,
                close: base,
                volume: None,
            });
        }
        app.world_mut()
            .resource_mut::<InstrumentTradingDataMap>()
            .map
            .insert("T".to_string(), data);

        let price_gutter = app
            .world_mut()
            .spawn((PriceGutter, Transform::default()))
            .id();
        let time_gutter = app
            .world_mut()
            .spawn((TimeGutter, Transform::default()))
            .id();
        // base_price_y / cell_height を実値で固定し、autoscale を切って毎フレーム Changed を作る。
        let mut state = ChartViewState::default();
        state.auto_scale = false;
        state.base_price_y = 205.0;
        state.cell_height = 2.0;
        state.latest_x = 9 * 60_000;
        let chart = app
            .world_mut()
            .spawn((
                state,
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                PriceGutterRef(price_gutter),
                TimeGutterRef(time_gutter),
            ))
            .id();

        // 毎フレーム ChartViewState を「同値で」touch して Changed を立てる system。
        app.add_systems(
            Update,
            (
                move |mut q: Query<&mut ChartViewState>| {
                    if let Ok(mut s) = q.get_mut(chart) {
                        s.set_changed();
                    }
                },
                price_axis_labels_system,
                time_axis_labels_system,
            )
                .chain(),
        );

        app.update();
        let count_after_1 = {
            let world = app.world_mut();
            let mut pq = world.query::<&PriceLabel>();
            pq.iter(world).count()
        };
        app.update();
        app.update();
        let count_after_3 = {
            let world = app.world_mut();
            let mut pq = world.query::<&PriceLabel>();
            pq.iter(world).count()
        };
        assert!(count_after_1 > 0);
        assert_eq!(
            count_after_1, count_after_3,
            "labels must be replaced each frame, not accumulated"
        );
    }

    /// gutter が despawn 済 (chart panel が prune→sync で teardown 中) でも panic しないこと。
    /// 実機の `set_parent` to despawned entity panic (bevy_hierarchy child_builder.rs:173) の回帰防止。
    #[test]
    fn axis_systems_skip_despawned_gutter_without_panic() {
        use crate::trading::{InstrumentTradingData, InstrumentTradingDataMap, OhlcPoint};
        use crate::ui::chart_viewstate::{ChartViewState, RequestAutoscale};

        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.add_message::<RequestAutoscale>();
        app.init_resource::<InstrumentTradingDataMap>();

        let mut data = InstrumentTradingData::default();
        for i in 0..10i64 {
            let base = 100.0 + i as f32;
            data.ohlc_points.push(OhlcPoint {
                timestamp_ms: i * 60_000,
                open_time_ms: i * 60_000,
                open: base,
                high: base + 1.0,
                low: base - 1.0,
                close: base,
                volume: None,
            });
        }
        app.world_mut()
            .resource_mut::<InstrumentTradingDataMap>()
            .map
            .insert("T".to_string(), data);

        // gutter を spawn してから即 despawn し、ref だけ chart に残す (teardown レース再現)。
        let price_gutter = app
            .world_mut()
            .spawn((PriceGutter, Transform::default()))
            .id();
        let time_gutter = app
            .world_mut()
            .spawn((TimeGutter, Transform::default()))
            .id();
        app.world_mut().entity_mut(price_gutter).despawn();
        app.world_mut().entity_mut(time_gutter).despawn();

        app.world_mut().spawn((
            ChartViewState::default(),
            ChartInstrument {
                instrument_id: "T".to_string(),
            },
            PriceGutterRef(price_gutter),
            TimeGutterRef(time_gutter),
        ));

        app.add_systems(Update, (price_axis_labels_system, time_axis_labels_system));

        // panic せず完走すれば OK。ラベルは 0 件 (gutter 不在で skip)。
        app.update();
        app.update();

        let world = app.world_mut();
        let mut pq = world.query::<&PriceLabel>();
        let mut tq = world.query::<&TimeLabel>();
        assert_eq!(pq.iter(world).count(), 0, "no price labels for dead gutter");
        assert_eq!(tq.iter(world).count(), 0, "no time labels for dead gutter");
    }
}
