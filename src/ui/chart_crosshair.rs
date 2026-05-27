//! Chart の crosshair (十字線 + price/time readout badge) (Phase 7.3 Phase D)。
//!
//! flowsurface (`.claude/skills/flowsurface/src/src/chart.rs`) の `crosshair` 描画と
//! `clear_crosshair` を Bevy に翻訳する。flowsurface は crosshair-only の変化で main geometry
//! Cache を再生成しない設計 (`Caches::crosshair` 層が独立)。本実装も同じ責務分割を
//! スケジューラ層で実現する:
//!
//! - **observer** (`Pointer<Move>`/`<Out>`): `cursor_world` だけを書く。`hovered_price`/
//!   `hovered_time_ms` は計算しない (Caveat #28: observer は `ChartSet::Autoscale` の前後
//!   どちらで発火するか保証されないので、stale な `base_price_y`/`cell_height` で readout が
//!   1 フレーム古くなる)。observer は schedule 外なので `ChartSet` に含めない。
//! - **`chart_crosshair_derive_system`** (`ChartSet::Render`, autoscale 確定後): autoscale 後の
//!   確定値で `hovered_price`/`hovered_time_ms` を計算する。`Or<(Changed<CrosshairState>,
//!   Changed<ChartViewState>)>` で「cursor 動 or viewstate 動」どちらの起点でも再計算。
//!   DerefMut ガードで同値代入を避け self-`Changed` ループを断つ (Caveat #29 と同根)。
//! - **`chart_crosshair_render_system`** (`ChartSet::Render`): 毎フレーム純 draw。ShapePainter は
//!   immediate-mode なので `Changed` で gate しない (Caveat #11)。描画スキップは
//!   `cursor_world.is_none()` の per-entity continue で行う。
//! - **`crosshair_badge_system`** (`ChartSet::Render`, `Changed<CrosshairState>` 駆動): hover 中の
//!   価格/時刻を gutter 内に強調表示する retained Text2d。axis label と同じ despawn+respawn
//!   パターン (Caveat #26: `despawn` は子孫を消さないので個別 / `despawn_recursive`)。
//!   z は cross line +0.5 の上の +0.6 (axis label +0.3 — Caveat #16)。

use crate::trading::{InstrumentTradingDataMap, OhlcPoint};
use crate::ui::chart_axes::{
    PriceGutter, PriceGutterRef, TimeGutter, TimeGutterRef, format_time_label,
};
use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::chart_volume::format_volume;
use crate::ui::components::ChartInstrument;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy_vector_shapes::prelude::*;

// ─── Component ───

/// 1 chart entity あたりの crosshair 状態。chart entity spawn 時に `default()` を一緒に挿入。
#[derive(Component, Default)]
pub struct CrosshairState {
    /// chart-local 座標系の cursor 位置 (`None` = hover 外)。observer が書く。
    pub cursor_world: Option<Vec2>,
    /// hover 行の価格 (main area 内のみ `Some`)。derive system が書く。
    pub hovered_price: Option<f32>,
    /// hover 列の時刻 (ms)。derive system が書く。
    pub hovered_time_ms: Option<i64>,
    /// hover 中の最近傍 candle の volume (volume area 内のみ `Some`)。derive system が書く。
    /// `hovered_price` とは排他 (同時に `Some` にならない)。
    pub hovered_volume: Option<f32>,
}

/// crosshair の price/time badge。どの chart のものかで despawn 対象を絞る。
#[derive(Component)]
pub struct CrosshairBadge {
    pub target_chart: Entity,
}

// ─── 描画定数 ───

/// cross line の z (Caveat #16: axis label +0.3、badge +0.6 の中間)。
const CROSS_LINE_Z: f32 = 0.5;
/// badge 背景の z (axis label +0.3 / cross line +0.5 より上 — Caveat #16)。
const BADGE_Z: f32 = 0.6;
/// cross line の色 (薄いグレー半透明)。
const CROSS_LINE_COLOR: Color = Color::srgba(0.8, 0.8, 0.8, 0.5);
/// badge 背景色 (bluish accent、gutter のラベルより目立たせる)。
const BADGE_BG_COLOR: Color = Color::srgba(0.18, 0.42, 0.58, 0.95);
/// badge テキスト色。
const BADGE_TEXT_COLOR: Color = Color::WHITE;
/// badge テキストサイズ (axis label と同じ)。
const BADGE_TEXT_SIZE: f32 = 11.0;
/// price badge 背景の高さ。
const PRICE_BADGE_HEIGHT: f32 = 16.0;
/// time badge 背景の幅 ("HH:MM" + 余白)。
const TIME_BADGE_WIDTH: f32 = 46.0;

use crate::ui::chart_viewstate::{PRICE_GUTTER_WIDTH, TIME_GUTTER_HEIGHT};

// ─── observer (schedule 外、Caveat #28) ───

/// 新しく spawn された chart entity (`CrosshairState` + `Sprite`) に `Pointer<Move>`/`<Out>`
/// observer を貼る。`Added<CrosshairState>` は spawn 次フレームで一度だけ true になる。
/// observer は対象 entity 自身に貼るので chart despawn と同時に消える (set_parent panic 無縁)。
pub fn install_chart_crosshair_observer(
    mut commands: Commands,
    new_charts: Query<Entity, (Added<CrosshairState>, With<Sprite>)>,
) {
    for entity in &new_charts {
        commands.entity(entity).observe(
            |trigger: On<Pointer<Move>>,
             mut chart_q: Query<(&GlobalTransform, &mut CrosshairState)>| {
                let Ok((gt, mut crosshair)) = chart_q.get_mut(trigger.entity) else {
                    return;
                };
                // `hit.position` は world space (bevy_sprite_picking_backend 前提 — Caveat #12/#24)。
                // chart GlobalTransform は scale=1 (translation のみ) なので引き算で chart-local 化。
                // observer は ChartSet::Autoscale 順序非依存にするため ChartViewState を読まない。
                // position が None の Move は位置不明なので skip する (旧 unwrap_or(ZERO) は world
                // 原点 - chart 位置という garbage 座標に crosshair を飛ばすので使わない)。
                let Some(world_pos) = trigger.hit.position else {
                    return;
                };
                let local = world_pos - gt.translation();
                crosshair.cursor_world = Some(local.xy());
                // hovered_price / hovered_time_ms は touch しない (Render system が計算)。
            },
        );
        commands.entity(entity).observe(
            |trigger: On<Pointer<Out>>, mut chart_q: Query<&mut CrosshairState>| {
                if let Ok(mut crosshair) = chart_q.get_mut(trigger.entity) {
                    crosshair.cursor_world = None;
                    crosshair.hovered_price = None;
                    crosshair.hovered_time_ms = None;
                    crosshair.hovered_volume = None;
                }
            },
        );
    }
}

// ─── derive (ChartSet::Render, autoscale 確定後) ───

/// autoscale 確定後の派生量で `hovered_price` / `hovered_time_ms` を確定する。
///
/// ⚠️ `Changed<CrosshairState>` 単独だと cursor 静止中に autoscale で `base_price_y`/`cell_height`
/// が動いた frame で `hovered_price` が stale になる。`Or<(Changed<CrosshairState>,
/// Changed<ChartViewState>)>` で「cursor 動 or viewstate 動」どちらの起点でも再計算する。
pub fn chart_crosshair_derive_system(
    map: Res<InstrumentTradingDataMap>,
    mut chart_q: Query<
        (&ChartViewState, &ChartInstrument, &mut CrosshairState),
        Or<(Changed<CrosshairState>, Changed<ChartViewState>)>,
    >,
) {
    for (state, instrument, mut crosshair) in &mut chart_q {
        let Some(c) = crosshair.cursor_world else {
            continue; // Out observer 側で既に None 化済み
        };
        let new_t = state.x_to_time_ms(c.x);
        // ⚠️ hovered_price は main_area 内のみ計算する。volume area (`y < main_area_y_bottom()`) の
        //    y を y_to_price に渡すと base_price_y 以下に外挿された偽の価格が badge に出る。
        let in_main = c.y >= state.main_area_y_bottom();
        let new_p = if in_main {
            Some(state.y_to_price(c.y))
        } else {
            None
        };
        // volume area のときだけ最近傍 candle の volume を入れる (price とは排他 if/else)。
        let new_vol = if in_main {
            None
        } else {
            map.map
                .get(&instrument.instrument_id)
                .and_then(|data| nearest_candle_volume(&data.ohlc_points, new_t))
        };
        // DerefMut 抑制ガード (Caveat #29 と同根: 同値代入で Changed を立てると derive が再発火し続ける)。
        if crosshair.hovered_price != new_p {
            crosshair.hovered_price = new_p;
        }
        if crosshair.hovered_time_ms != Some(new_t) {
            crosshair.hovered_time_ms = Some(new_t);
        }
        if crosshair.hovered_volume != new_vol {
            crosshair.hovered_volume = new_vol;
        }
    }
}

/// `open_time_ms` 昇順の candle 列から `target_ms` に最も近い candle の volume を返す。
/// 該当 candle の volume が `None` ならそのまま `None`。空列でも `None`。
fn nearest_candle_volume(ohlc: &[OhlcPoint], target_ms: i64) -> Option<f32> {
    if ohlc.is_empty() {
        return None;
    }
    let idx = match ohlc.binary_search_by_key(&target_ms, |c| c.open_time_ms) {
        Ok(i) => i,
        Err(i) => {
            // i は挿入位置。前後どちらの candle が近いか距離で選ぶ。
            if i == 0 {
                0
            } else if i >= ohlc.len() {
                ohlc.len() - 1
            } else {
                let prev_dist = target_ms - ohlc[i - 1].open_time_ms;
                let next_dist = ohlc[i].open_time_ms - target_ms;
                if prev_dist <= next_dist { i - 1 } else { i }
            }
        }
    };
    ohlc[idx].volume
}

// ─── render (ChartSet::Render, 毎フレーム) ───

/// 十字線を毎フレーム描く。`Changed` フィルタを付けてはいけない (ShapePainter は immediate-mode、
/// 変化が無いフレームで line を発行しないと cross line が消える — Caveat #11)。
pub fn chart_crosshair_render_system(
    mut painter: ShapePainter,
    chart_q: Query<(&GlobalTransform, &ChartViewState, &CrosshairState)>,
) {
    for (gt, state, crosshair) in &chart_q {
        let Some(cursor) = crosshair.cursor_world else {
            continue;
        };
        painter.set_translation(gt.translation());
        painter.color = CROSS_LINE_COLOR;
        painter.thickness = 1.0;
        // 縦線 (cursor.x 上の price 軸方向)。
        painter.line(
            Vec3::new(cursor.x, -state.bounds.y / 2.0, CROSS_LINE_Z),
            Vec3::new(cursor.x, state.bounds.y / 2.0, CROSS_LINE_Z),
        );
        // 横線 (cursor.y 上の time 軸方向)。
        painter.line(
            Vec3::new(-state.bounds.x / 2.0, cursor.y, CROSS_LINE_Z),
            Vec3::new(state.bounds.x / 2.0, cursor.y, CROSS_LINE_Z),
        );
    }
}

// ─── badge (ChartSet::Render, Changed<CrosshairState> 駆動の retained Text2d) ───

/// hover 中の price/time を gutter 内に強調表示する。`Changed<CrosshairState>` のフレームのみ走る。
///
/// axis label と同じく despawn+respawn する。`despawn` は子孫を消さない (Caveat #26) ので、
/// 背景 Sprite + Text2d 子の 2 entity を `despawn_recursive` で一括 despawn する。
/// gutter が despawn 済 (chart panel teardown 中) の set_parent panic を生存ガードで防ぐ。
pub fn crosshair_badge_system(
    mut commands: Commands,
    chart_q: Query<
        (
            Entity,
            &ChartViewState,
            &CrosshairState,
            &PriceGutterRef,
            &TimeGutterRef,
        ),
        (With<ChartInstrument>, Changed<CrosshairState>),
    >,
    existing: Query<(Entity, &CrosshairBadge)>,
    live_price_gutter: Query<(), With<PriceGutter>>,
    live_time_gutter: Query<(), With<TimeGutter>>,
) {
    for (chart_entity, state, crosshair, price_ref, time_ref) in &chart_q {
        // 自 chart の既存 badge を一括 despawn (背景 + 文字の 2 entity)。
        for (badge_e, badge) in &existing {
            if badge.target_chart == chart_entity {
                commands.entity(badge_e).despawn_recursive();
            }
        }

        // price badge — main area 内 hover のときだけ (hovered_price = Some)。
        if let Some(price) = crosshair.hovered_price {
            if live_price_gutter.contains(price_ref.0) {
                let y = state.price_to_y(price);
                spawn_badge(
                    &mut commands,
                    chart_entity,
                    price_ref.0,
                    Vec2::new(0.0, y),
                    Vec2::new(PRICE_GUTTER_WIDTH, PRICE_BADGE_HEIGHT),
                    format!("{:.*}", state.decimals, price),
                );
            }
        }

        // volume badge — volume area hover のときだけ (hovered_volume = Some、price とは排他)。
        // price gutter (右軸) の cursor.y 行に出す (price badge と対称: horizontal cross line 上)。
        if let Some((vol, cursor)) = crosshair.hovered_volume.zip(crosshair.cursor_world) {
            if live_price_gutter.contains(price_ref.0) {
                spawn_badge(
                    &mut commands,
                    chart_entity,
                    price_ref.0,
                    Vec2::new(0.0, cursor.y),
                    Vec2::new(PRICE_GUTTER_WIDTH, PRICE_BADGE_HEIGHT),
                    format_volume(vol),
                );
            }
        }

        // time badge — hover 中なら常時 (hovered_time_ms = Some)。
        if let Some(t) = crosshair.hovered_time_ms {
            if live_time_gutter.contains(time_ref.0) {
                if let Some(text) = format_time_label(t) {
                    let x = state.interval_to_x(t);
                    spawn_badge(
                        &mut commands,
                        chart_entity,
                        time_ref.0,
                        Vec2::new(x, 0.0),
                        Vec2::new(TIME_BADGE_WIDTH, TIME_GUTTER_HEIGHT),
                        text,
                    );
                }
            }
        }
    }
}

/// 背景 Sprite (`CrosshairBadge` マーカー) + 中央寄せ Text2d を gutter の子として spawn する。
/// `gutter_local` は gutter 原点からのローカル座標 (axis label と同じ慣例: gutter は chart-local
/// y=0 / x=0 に置かれているので price_to_y / interval_to_x の結果をそのまま渡せる)。
fn spawn_badge(
    commands: &mut Commands,
    target_chart: Entity,
    gutter: Entity,
    gutter_local: Vec2,
    bg_size: Vec2,
    text: String,
) {
    let text_entity = commands
        .spawn((
            Text2d::new(text),
            TextFont {
                font_size: BADGE_TEXT_SIZE,
                ..default()
            },
            TextColor(BADGE_TEXT_COLOR),
            Anchor::Center,
            // 背景 Sprite の子。z をわずかに上げて文字を背景の上に出す。
            Transform::from_xyz(0.0, 0.0, 0.01),
        ))
        .id();
    let badge = commands
        .spawn((
            Sprite {
                color: BADGE_BG_COLOR,
                custom_size: Some(bg_size),
                ..default()
            },
            Transform::from_xyz(gutter_local.x, gutter_local.y, BADGE_Z),
            CrosshairBadge { target_chart },
        ))
        .id();
    commands.entity(badge).add_child(text_entity);
    commands.entity(badge).set_parent(gutter);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chart_axes::{PriceGutter, TimeGutter};

    fn state_for_hover() -> ChartViewState {
        let mut s = ChartViewState::default();
        s.auto_scale = false;
        s.base_price_y = 100.0;
        s.cell_height = 2.0;
        s.cell_width = 6.0;
        s.latest_x = 600_000;
        s
    }

    /// main area 内 cursor → hovered_price/time が round-trip 整合する。
    #[test]
    fn derive_computes_price_and_time_in_main_area() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        let state = state_for_hover();
        // main area 中央付近 (y >= main_area_y_bottom)。
        let cursor = Vec2::new(20.0, 15.0);
        assert!(cursor.y >= state.main_area_y_bottom());
        let expected_price = state.y_to_price(cursor.y);
        let expected_time = state.x_to_time_ms(cursor.x);

        let chart = app
            .world_mut()
            .spawn((
                state,
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState {
                    cursor_world: Some(cursor),
                    ..default()
                },
            ))
            .id();
        app.add_systems(Update, chart_crosshair_derive_system);
        app.update();

        let cs = app.world().entity(chart).get::<CrosshairState>().unwrap();
        let hp = cs.hovered_price.expect("price in main area");
        assert!(
            (hp - expected_price).abs() < 1e-2,
            "price {hp} vs {expected_price}"
        );
        assert_eq!(cs.hovered_time_ms, Some(expected_time));
    }

    /// volume area (main_area_y_bottom 未満) の cursor → hovered_price=None、time は Some。
    #[test]
    fn derive_price_none_in_volume_area() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        let state = state_for_hover();
        // volume area (下 20%): y < main_area_y_bottom。
        let cursor = Vec2::new(10.0, state.main_area_y_bottom() - 5.0);
        let expected_time = state.x_to_time_ms(cursor.x);

        let chart = app
            .world_mut()
            .spawn((
                state,
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState {
                    cursor_world: Some(cursor),
                    ..default()
                },
            ))
            .id();
        app.add_systems(Update, chart_crosshair_derive_system);
        app.update();

        let cs = app.world().entity(chart).get::<CrosshairState>().unwrap();
        assert_eq!(cs.hovered_price, None, "no price in volume area");
        assert_eq!(cs.hovered_time_ms, Some(expected_time));
    }

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

    /// volume area の cursor → 最近傍 candle の volume が hovered_volume に入り、price は None。
    #[test]
    fn derive_computes_volume_in_volume_area() {
        use crate::trading::InstrumentTradingData;

        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();

        let state = state_for_hover();
        // 600_000 の candle (vol 250) に重なる cursor.x を round-trip で求める。
        let cursor_x = state.interval_to_x(600_000);
        let cursor = Vec2::new(cursor_x, state.main_area_y_bottom() - 5.0);
        assert!(cursor.y < state.main_area_y_bottom());

        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "T".to_string(),
                InstrumentTradingData {
                    ohlc_points: vec![
                        ohlc_vol(540_000, Some(100.0)),
                        ohlc_vol(600_000, Some(250.0)),
                    ],
                    ..Default::default()
                },
            );
        }

        let chart = app
            .world_mut()
            .spawn((
                state,
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState {
                    cursor_world: Some(cursor),
                    ..default()
                },
            ))
            .id();
        app.add_systems(Update, chart_crosshair_derive_system);
        app.update();

        let cs = app.world().entity(chart).get::<CrosshairState>().unwrap();
        assert_eq!(cs.hovered_volume, Some(250.0), "nearest candle volume");
        assert_eq!(cs.hovered_price, None, "price is None in volume area");
    }

    #[test]
    fn nearest_candle_volume_picks_closest() {
        let candles = [
            ohlc_vol(0, Some(10.0)),
            ohlc_vol(60_000, Some(20.0)),
            ohlc_vol(120_000, Some(30.0)),
        ];
        // 完全一致。
        assert_eq!(nearest_candle_volume(&candles, 60_000), Some(20.0));
        // 中間 (20_000 は 0 に近い)。
        assert_eq!(nearest_candle_volume(&candles, 20_000), Some(10.0));
        // 中間 (50_000 は 60_000 に近い)。
        assert_eq!(nearest_candle_volume(&candles, 50_000), Some(20.0));
        // 範囲外 (左)。
        assert_eq!(nearest_candle_volume(&candles, -100_000), Some(10.0));
        // 範囲外 (右)。
        assert_eq!(nearest_candle_volume(&candles, 999_999), Some(30.0));
    }

    #[test]
    fn nearest_candle_volume_empty_and_none() {
        assert_eq!(nearest_candle_volume(&[], 0), None);
        // 最近傍 candle の volume が None なら None。
        let candles = [ohlc_vol(0, None)];
        assert_eq!(nearest_candle_volume(&candles, 0), None);
    }

    /// cursor_world=None (hover 外) では derive は readout を触らない。
    #[test]
    fn derive_noop_when_cursor_none() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        let chart = app
            .world_mut()
            .spawn((
                state_for_hover(),
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState::default(), // cursor_world: None
            ))
            .id();
        app.add_systems(Update, chart_crosshair_derive_system);
        app.update();

        let cs = app.world().entity(chart).get::<CrosshairState>().unwrap();
        assert_eq!(cs.hovered_price, None);
        assert_eq!(cs.hovered_time_ms, None);
    }

    /// Caveat #29: cursor 静止中は CrosshairState が変化し続けない (DerefMut ガードで収束)。
    #[test]
    fn derive_converges_within_few_frames() {
        #[derive(Resource, Default)]
        struct ChangedLog(Vec<usize>);

        let mut app = App::new();
        app.init_resource::<ChangedLog>();
        app.init_resource::<InstrumentTradingDataMap>();
        app.world_mut().spawn((
            state_for_hover(),
            ChartInstrument {
                instrument_id: "T".to_string(),
            },
            CrosshairState {
                cursor_world: Some(Vec2::new(20.0, 15.0)),
                ..default()
            },
        ));
        app.add_systems(
            Update,
            (
                chart_crosshair_derive_system,
                |q: Query<(), Changed<CrosshairState>>, mut log: ResMut<ChangedLog>| {
                    log.0.push(q.iter().count());
                },
            )
                .chain(),
        );

        for _ in 0..5 {
            app.update();
        }

        let log = &app.world().resource::<ChangedLog>().0;
        assert_eq!(log.len(), 5);
        for (i, &c) in log.iter().enumerate() {
            if i >= 2 {
                assert_eq!(
                    c,
                    0,
                    "frame {} still mutating CrosshairState (log={:?})",
                    i + 1,
                    log
                );
            }
        }
    }

    /// badge が gutter 子として spawn され、target_chart が正しいこと。
    #[test]
    fn badge_system_spawns_badges_as_gutter_children() {
        let mut app = App::new();

        let price_gutter = app
            .world_mut()
            .spawn((PriceGutter, Transform::default()))
            .id();
        let time_gutter = app
            .world_mut()
            .spawn((TimeGutter, Transform::default()))
            .id();

        let mut state = state_for_hover();
        state.decimals = 2;
        let chart = app
            .world_mut()
            .spawn((
                state,
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState {
                    cursor_world: Some(Vec2::new(10.0, 15.0)),
                    hovered_price: Some(101.5),
                    hovered_time_ms: Some(540_000),
                    hovered_volume: None,
                },
                PriceGutterRef(price_gutter),
                TimeGutterRef(time_gutter),
            ))
            .id();

        app.add_systems(Update, crosshair_badge_system);
        app.update();

        let world = app.world_mut();
        let mut bq = world.query::<(&CrosshairBadge, &Parent)>();
        let badges: Vec<_> = bq.iter(world).collect();
        // price badge + time badge = 2 (どちらも gutter 子)。
        assert_eq!(badges.len(), 2, "expected price + time badge");
        for (badge, parent) in &badges {
            assert_eq!(badge.target_chart, chart);
            let p = parent.get();
            assert!(
                p == price_gutter || p == time_gutter,
                "badge must be child of a gutter"
            );
        }
    }

    /// 連続した Changed フレームで badge が累積せず置き換わること。
    #[test]
    fn badge_replaces_not_accumulates() {
        let mut app = App::new();

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
                state_for_hover(),
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState {
                    cursor_world: Some(Vec2::new(10.0, 15.0)),
                    hovered_price: Some(101.5),
                    hovered_time_ms: Some(540_000),
                    hovered_volume: None,
                },
                PriceGutterRef(price_gutter),
                TimeGutterRef(time_gutter),
            ))
            .id();

        // 毎フレーム CrosshairState を touch して Changed を立てる。
        app.add_systems(
            Update,
            (
                move |mut q: Query<&mut CrosshairState>| {
                    if let Ok(mut s) = q.get_mut(chart) {
                        s.set_changed();
                    }
                },
                crosshair_badge_system,
            )
                .chain(),
        );

        app.update();
        let count_1 = {
            let world = app.world_mut();
            let mut bq = world.query::<&CrosshairBadge>();
            bq.iter(world).count()
        };
        app.update();
        app.update();
        let count_3 = {
            let world = app.world_mut();
            let mut bq = world.query::<&CrosshairBadge>();
            bq.iter(world).count()
        };
        assert_eq!(count_1, 2);
        assert_eq!(count_1, count_3, "badges must replace, not accumulate");
    }

    /// volume area hover: price badge は出ず、volume + time の 2 badge が gutter 子になる。
    #[test]
    fn badge_volume_area_spawns_volume_and_time_badges() {
        let mut app = App::new();

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
                state_for_hover(),
                ChartInstrument {
                    instrument_id: "T".to_string(),
                },
                CrosshairState {
                    cursor_world: Some(Vec2::new(10.0, -80.0)),
                    hovered_price: None,
                    hovered_time_ms: Some(540_000),
                    hovered_volume: Some(1234.0),
                },
                PriceGutterRef(price_gutter),
                TimeGutterRef(time_gutter),
            ))
            .id();

        app.add_systems(Update, crosshair_badge_system);
        app.update();

        let world = app.world_mut();
        let mut bq = world.query::<(&CrosshairBadge, &Parent)>();
        let badges: Vec<_> = bq.iter(world).collect();
        assert_eq!(
            badges.len(),
            2,
            "expected volume + time badge (no price badge)"
        );
        for (badge, parent) in &badges {
            assert_eq!(badge.target_chart, chart);
            let p = parent.get();
            assert!(p == price_gutter || p == time_gutter);
        }
    }

    /// gutter が despawn 済でも set_parent panic せず、badge 0 件で完走すること。
    #[test]
    fn badge_skips_despawned_gutter_without_panic() {
        let mut app = App::new();

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
            state_for_hover(),
            ChartInstrument {
                instrument_id: "T".to_string(),
            },
            CrosshairState {
                cursor_world: Some(Vec2::new(10.0, 15.0)),
                hovered_price: Some(101.5),
                hovered_time_ms: Some(540_000),
                hovered_volume: None,
            },
            PriceGutterRef(price_gutter),
            TimeGutterRef(time_gutter),
        ));

        app.add_systems(Update, crosshair_badge_system);
        app.update();
        app.update();

        let world = app.world_mut();
        let mut bq = world.query::<&CrosshairBadge>();
        assert_eq!(bq.iter(world).count(), 0, "no badges for dead gutter");
    }
}
