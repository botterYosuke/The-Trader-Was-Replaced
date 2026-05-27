//! Chart の pan (drag) / zoom (mouse wheel) インタラクション (Phase C)。
//!
//! flowsurface (`.claude/skills/flowsurface/src/src/chart.rs`) の `canvas_interaction` /
//! `Message::Scaled` の cursor 中心ズームを Bevy に翻訳したもの。
//!
//! - **Pan**: chart Sprite に貼った `Pointer<Drag>` observer が `translation` を動かす。
//!   WindowRoot 側の title bar drag (window 移動) と混線しないよう `propagate(false)` する
//!   (Caveat #2)。
//! - **Zoom**: Bevy 0.15 picking には `Pointer<Scroll>` が無い (Caveat #22) ので
//!   `EventReader<MouseWheel>` + `HoverMap` で hover 中の chart entity を引き、
//!   `Camera::viewport_to_world_2d` で cursor を world へ投影して cursor 中心ズームする。
//!
//! 両系統とも `auto_scale = false` にする (pan/zoom 開始で autoscale を切る)。これにより
//! `chart_interaction_tick_system` (Changed<ChartViewState> reader) は autoscale を再要求せず、
//! ユーザの pan/zoom が次フレームの autoscale で巻き戻されない。
//!
//! observer は `Update` schedule の外で event-driven 発火するので `ChartSet` に入れない
//! (Caveat #28)。`chart_scroll_zoom_system` は regular system なので `ChartSet::Interaction`。

use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::components::ChartInstrument;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerId, PointerLocation};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// double-click とみなす連続クリックの最大間隔 (秒)。OS 標準の ~0.5s に合わせる。
const DOUBLE_CLICK_SECS: f32 = 0.4;

/// double-click reset (Phase E) 用の per-chart クリック状態。
///
/// ⚠️ Bevy 0.15 picking は drag 後の pointer up でも `Pointer<Click>` を発火する
/// (`bevy_picking` `pointer_events`)。pan ドラッグ 2 連発を double-click と誤検出しないよう、
/// drag observer が `dragged` に印を付け、click observer はその印があるクリックを「genuine click
/// ではない」として double-click 列から除外する。
#[derive(Resource, Default)]
pub struct ChartClickState {
    /// entity → 直近の genuine click の時刻 (秒)。
    last_click: HashMap<Entity, f32>,
    /// press 以降に drag が発生した chart。click 時に印があれば click 列をリセットして無視する。
    dragged: HashSet<Entity>,
}

/// `last` (直近 genuine click 時刻) からの経過が閾値以内なら double-click。`last == None` は単発。
fn is_double_click(last: Option<f32>, now: f32) -> bool {
    matches!(last, Some(t) if now - t <= DOUBLE_CLICK_SECS)
}

/// ホイール 1 ノッチあたりのズーム強度。値が大きいほど 1 ノッチの倍率変化が小さい。
const ZOOM_SENSITIVITY: f32 = 30.0;
/// `MouseScrollUnit::Pixel` を Line 単位へ概算する除数 (OS/デバイスで桁が違う、実機 tuning 暫定値)。
const PIXELS_PER_LINE: f32 = 20.0;
/// cell 幅 (時間軸) のクランプ域 (px/candle)。下限は潰れ防止、上限は 1 本が画面を埋める過剰ズーム防止。
const MIN_CELL_WIDTH: f32 = 1.0;
const MAX_CELL_WIDTH: f32 = 50.0;
/// cell 高さ (価格軸) のクランプは price 1 単位あたりの px で持つ。
///
/// ⚠️ `cell_height` を**絶対値**でクランプしてはいけない。`price_to_y` は
/// `(price - base) / tick_size * cell_height` なので実効スケールは `cell_height / tick_size`
/// (= px / price-unit) であり、`cell_height` の絶対桁は `tick_size` (現状 0.01 ハードコード) に
/// 比例して動く。絶対 `[0.1, 1000]` で挟むと、autoscale が高価格銘柄で吐く `cell_height << 0.1`
/// (例: ¥2000 株 range¥48 → 144*0.01/48 ≈ 0.03) に対し最初のホイールが clamp 床へ貼り付き
/// 縦倍率が不連続に 3〜100x ジャンプする (¥500 超の TSE 銘柄でほぼ必ず発生)。px/price-unit で
/// 持てば `tick_size` 非依存になり autoscale 出力も将来の per-instrument tick もクランプに刺さらない。
const MIN_PX_PER_PRICE_UNIT: f32 = 0.001;
const MAX_PX_PER_PRICE_UNIT: f32 = 2000.0;

/// 新しく spawn された chart entity (`ChartViewState` + `Sprite`) に pan 用の
/// `Pointer<Drag>` observer を貼る。
///
/// `Added<ChartViewState>` は spawn の次フレームで一度だけ true になる。observer は対象 entity
/// 自身に貼るので、後で chart が despawn されても observer は一緒に消える (set_parent panic とは無縁)。
pub fn install_chart_drag_observer(
    mut commands: Commands,
    new_charts: Query<Entity, (Added<ChartViewState>, With<Sprite>)>,
) {
    for entity in &new_charts {
        commands.entity(entity).observe(
            |mut drag: On<Pointer<Drag>>,
             mut chart_q: Query<&mut ChartViewState>,
             mut click_state: ResMut<ChartClickState>,
             // camera scale 補正 (規約 5 / floating_window.rs:117): bevy_pancam のズーム状態でも
             // world-space の pan 量が screen-space の drag 距離と一致するように scale を掛ける。
             camera_q: Query<&Projection, With<Camera2d>>| {
                // ⚠️ Pointer<Drag> は全ボタンで発火する (bevy_picking 0.15 `pointer_events` が
                //    `for button in PointerButton::iter()`)。右/中ボタンドラッグは camera.rs の
                //    suppression が「PanCam でキャンバスをパン」と定義しているので、ここで処理すると
                //    chart も同時にパンし auto_scale まで切れて二重挙動になる。左ボタンのみ chart pan。
                if drag.button != PointerButton::Primary {
                    return;
                }
                let entity = drag.entity;
                let Ok(mut state) = chart_q.get_mut(entity) else {
                    return;
                };
                let delta = drag.delta;
                let scale = camera_q.get_single().map(|p| {
                    if let Projection::Orthographic(proj) = p { proj.scale } else { 1.0 }
                }).unwrap_or(1.0);
                state.translation.x += delta.x * scale;
                state.translation.y -= delta.y * scale; // Bevy Y は上が正、Pointer delta は下が正
                state.auto_scale = false; // pan 開始で autoscale off
                // この press はドラッグ。直後の Pointer<Click> を double-click 列から除外する印。
                click_state.dragged.insert(entity);
                drag.propagate(false); // title bar drag (window 移動) に bubble させない (Caveat #2)
            },
        );
    }
}

/// 新しく spawn された chart entity に double-click reset 用の `Pointer<Click>` observer を貼る。
///
/// flowsurface (`chart.rs::Message::DoubleClick`) の「軸ダブルクリックで autoscale 再 fit」に相当
/// (本実装は軸 gutter ではなく chart 本体のダブルクリックで pan/zoom を一括リセットする)。
/// pan/zoom で一度 `auto_scale=false` になると再有効化する手段が無い問題を解消する。
pub fn install_chart_autoscale_reset_observer(
    mut commands: Commands,
    new_charts: Query<Entity, (Added<ChartViewState>, With<Sprite>)>,
) {
    for entity in &new_charts {
        commands.entity(entity).observe(
            |mut click: On<Pointer<Click>>,
             time: Res<Time>,
             mut click_state: ResMut<ChartClickState>,
             mut chart_q: Query<&mut ChartViewState>| {
                if click.button != PointerButton::Primary {
                    return;
                }
                let entity = click.entity;
                click.propagate(false); // window 移動/他パネルに bubble させない (Caveat #2)
                // drag 由来の click は genuine click ではない (Bevy 0.15 は drag 後も Click 発火)。
                // 印を消し、double-click 列もリセットして「pan 2 連発 = reset」の誤検出を断つ。
                if click_state.dragged.remove(&entity) {
                    click_state.last_click.remove(&entity);
                    return;
                }
                let now = time.elapsed_secs();
                if is_double_click(click_state.last_click.get(&entity).copied(), now) {
                    click_state.last_click.remove(&entity);
                    if let Ok(mut state) = chart_q.get_mut(entity) {
                        state.reset_view();
                    }
                } else {
                    click_state.last_click.insert(entity, now);
                }
            },
        );
    }
}

/// despawn された chart の `ChartClickState` エントリを掃除する (entity key leak 防止)。
/// chart は instrument の入退場 (replay/live 切替) で spawn/despawn を繰り返すため、
/// `RemovedComponents<ChartViewState>` で消えた chart の last_click / dragged を除去する
/// (`instrument_chart_sync_system` が `InstrumentTradingDataMap` を掃除するのと同じ衛生)。
pub fn chart_click_state_cleanup_system(
    mut removed: RemovedComponents<ChartViewState>,
    mut click_state: ResMut<ChartClickState>,
) {
    for entity in removed.read() {
        click_state.last_click.remove(&entity);
        click_state.dragged.remove(&entity);
    }
}

/// cursor 位置を固定したまま `cell_width` / `cell_height` を `wheel_y` ノッチ分ズームする。
///
/// flowsurface `Message::Scaled` の cursor delta 補正の写経 (Caveat #10): ズーム前の cursor 直下の
/// price/time を保持し、ズーム後にその price/time が同じ chart-local 座標へ来るよう `translation`
/// を補正する。これが無いと「ズーム時に画面中央が動かない」挙動が出ない。
fn apply_cursor_zoom(state: &mut ChartViewState, cursor_local: Vec2, wheel_y: f32) {
    let cursor_price = state.y_to_price(cursor_local.y);
    let cursor_time = state.x_to_time_ms(cursor_local.x);
    let factor = 1.0 + wheel_y / ZOOM_SENSITIVITY;
    state.cell_width = (state.cell_width * factor).clamp(MIN_CELL_WIDTH, MAX_CELL_WIDTH);
    // tick_size 比例クランプ (上記コメント): px/price-unit を一定域に保ち tick_size 非依存にする。
    let min_cell_height = MIN_PX_PER_PRICE_UNIT * state.tick_size;
    let max_cell_height = MAX_PX_PER_PRICE_UNIT * state.tick_size;
    state.cell_height = (state.cell_height * factor).clamp(min_cell_height, max_cell_height);
    state.auto_scale = false;
    let new_cursor_y = state.price_to_y(cursor_price);
    let new_cursor_x = state.interval_to_x(cursor_time);
    state.translation.y -= new_cursor_y - cursor_local.y;
    state.translation.x -= new_cursor_x - cursor_local.x;
}

/// マウスホイールで hover 中の chart を cursor 中心ズームする (regular system, `ChartSet::Interaction`)。
pub fn chart_scroll_zoom_system(
    mut wheel: MessageReader<MouseWheel>,
    // Ctrl 押下中は「キャンバス全体ズーム」意図 (camera.rs の suppression と対称)。chart ズームは skip。
    keys: Res<ButtonInput<KeyCode>>,
    // Bevy 0.15.1: HoverMap は bevy::picking::focus (0.16+ で hover に rename)。
    hover_map: Res<HoverMap>,
    // hover_map で得た PointerId に対応する cursor 座標を引く。
    pointers: Query<(&PointerId, &PointerLocation)>,
    // multi-camera で get_single が silent-fail しないよう With<Camera2d> で絞る (Caveat #31)。
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut chart_q: Query<(&GlobalTransform, &mut ChartViewState), With<ChartInstrument>>,
) {
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        wheel.clear(); // Ctrl+ホイールはカメラズームに譲る。stale event を残さず捨てる
        return;
    }
    for ev in wheel.read() {
        // unit 正規化: Pixel は Line に概算する (OS/デバイスで桁が違う — Caveat 暫定値)。
        let y = match ev.unit {
            MouseScrollUnit::Line => ev.y,
            MouseScrollUnit::Pixel => ev.y / PIXELS_PER_LINE,
        };
        if y == 0.0 {
            continue;
        }
        // hover_map から chart_q にマッチする (entity, pointer_id) を採用。
        let Some((entity, ptr_id)) = hover_map
            .iter()
            .flat_map(|(ptr_id, set)| set.keys().map(move |e| (*e, *ptr_id)))
            .find(|(e, _)| chart_q.contains(*e))
        else {
            continue;
        };
        let Ok((cam, cam_t)) = camera_q.get_single() else {
            continue;
        };
        // hover 中の chart に対応する pointer の座標を引く (別 pointer の座標を拾わない)。
        let Some(loc) = pointers
            .iter()
            .find_map(|(id, p)| (*id == ptr_id).then(|| p.location.clone()).flatten())
        else {
            continue;
        };
        let Ok(world) = cam.viewport_to_world_2d(cam_t, loc.position) else {
            continue;
        };
        let Ok((gt, mut state)) = chart_q.get_mut(entity) else {
            continue;
        };
        // chart GlobalTransform は scale=1 (translation のみ) なので world delta == chart-local delta。
        let cursor_local = world - gt.translation().xy();
        apply_cursor_zoom(&mut state, cursor_local, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chart_viewstate::ChartViewState;

    fn state_for_zoom() -> ChartViewState {
        let mut s = ChartViewState::default();
        s.base_price_y = 100.0;
        s.cell_height = 2.0;
        s.cell_width = 6.0;
        s.latest_x = 600_000;
        s
    }

    /// cursor 直下の price/time がズーム後も同じ chart-local 座標に残る (flowsurface 流の中心固定)。
    #[test]
    fn cursor_centered_zoom_keeps_cursor_anchored() {
        let mut s = state_for_zoom();
        let cursor = Vec2::new(30.0, 20.0);
        let price_before = s.y_to_price(cursor.y);
        let time_before = s.x_to_time_ms(cursor.x);

        apply_cursor_zoom(&mut s, cursor, 1.0); // 1 ノッチズームイン

        assert!(
            s.cell_width > 6.0 && s.cell_height > 2.0,
            "zoom in must grow cells: w={} h={}",
            s.cell_width,
            s.cell_height
        );
        assert!(!s.auto_scale, "zoom must disable autoscale");
        assert!(
            (s.price_to_y(price_before) - cursor.y).abs() < 1e-2,
            "cursor price must stay anchored: {} vs {}",
            s.price_to_y(price_before),
            cursor.y
        );
        assert!(
            (s.interval_to_x(time_before) - cursor.x).abs() < 1e-1,
            "cursor time must stay anchored: {} vs {}",
            s.interval_to_x(time_before),
            cursor.x
        );
    }

    /// 反対方向 (ズームアウト) でも cursor が固定される。
    #[test]
    fn cursor_centered_zoom_out_keeps_cursor_anchored() {
        let mut s = state_for_zoom();
        let cursor = Vec2::new(-40.0, -15.0);
        let price_before = s.y_to_price(cursor.y);
        let time_before = s.x_to_time_ms(cursor.x);

        apply_cursor_zoom(&mut s, cursor, -1.0); // 1 ノッチズームアウト

        assert!(
            s.cell_width < 6.0 && s.cell_height < 2.0,
            "zoom out must shrink cells"
        );
        assert!((s.price_to_y(price_before) - cursor.y).abs() < 1e-2);
        assert!((s.interval_to_x(time_before) - cursor.x).abs() < 1e-1);
    }

    /// cell_width / cell_height が MIN/MAX でクランプされる (cell_height は tick_size 比例)。
    #[test]
    fn zoom_clamps_cell_dimensions() {
        let mut s = state_for_zoom();
        let max_h = MAX_PX_PER_PRICE_UNIT * s.tick_size;
        let min_h = MIN_PX_PER_PRICE_UNIT * s.tick_size;
        for _ in 0..500 {
            apply_cursor_zoom(&mut s, Vec2::ZERO, 5.0);
        }
        assert!(s.cell_width <= MAX_CELL_WIDTH + 1e-3, "w={}", s.cell_width);
        assert!(s.cell_height <= max_h + 1e-3, "h={}", s.cell_height);

        let mut s2 = state_for_zoom();
        for _ in 0..500 {
            apply_cursor_zoom(&mut s2, Vec2::ZERO, -5.0);
        }
        assert!(
            s2.cell_width >= MIN_CELL_WIDTH - 1e-3,
            "w={}",
            s2.cell_width
        );
        assert!(s2.cell_height >= min_h - 1e-9, "h={}", s2.cell_height);
    }

    /// 回帰: autoscale が高価格銘柄で吐く小さい cell_height (< 旧 MIN 0.1) でも、最初の 1 ノッチで
    /// クランプ床に貼り付いて縦倍率が不連続にジャンプしないこと。
    #[test]
    fn small_autoscaled_cell_height_does_not_snap_on_first_zoom() {
        let mut s = state_for_zoom();
        // ¥2000 株 range¥48 相当 (144*0.01/48 ≈ 0.03)。旧実装では clamp(0.1,..) で 0.1 へ跳ねた。
        s.cell_height = 0.03;
        apply_cursor_zoom(&mut s, Vec2::new(10.0, 10.0), 1.0); // 1 ノッチズームイン
        let factor = 1.0 + 1.0 / ZOOM_SENSITIVITY;
        // 連続: 期待値は 0.03*factor 付近 (床 0.1 へジャンプしない)。
        assert!(
            (s.cell_height - 0.03 * factor).abs() < 1e-4,
            "cell_height must scale continuously, got {}",
            s.cell_height
        );
    }

    #[test]
    fn is_double_click_window() {
        // 直近 click 無し → 単発。
        assert!(!is_double_click(None, 1.0));
        // 閾値内 → double。
        assert!(is_double_click(Some(1.0), 1.0 + DOUBLE_CLICK_SECS - 0.01));
        // 閾値ちょうど → double (<=)。
        assert!(is_double_click(Some(1.0), 1.0 + DOUBLE_CLICK_SECS));
        // 閾値超過 → 単発扱い。
        assert!(!is_double_click(Some(1.0), 1.0 + DOUBLE_CLICK_SECS + 0.01));
    }

    /// despawn された chart の click 状態が cleanup system で除去される (entity key leak 防止)。
    #[test]
    fn cleanup_removes_despawned_chart_click_state() {
        let mut app = App::new();
        app.init_resource::<ChartClickState>();
        app.add_systems(Update, chart_click_state_cleanup_system);

        let chart = app.world_mut().spawn(ChartViewState::default()).id();
        {
            let mut cs = app.world_mut().resource_mut::<ChartClickState>();
            cs.last_click.insert(chart, 1.0);
            cs.dragged.insert(chart);
        }
        app.update(); // RemovedComponents は空、エントリ保持。
        assert!(
            app.world()
                .resource::<ChartClickState>()
                .last_click
                .contains_key(&chart)
        );

        app.world_mut().entity_mut(chart).despawn();
        app.update(); // despawn を検出して掃除。

        let cs = app.world().resource::<ChartClickState>();
        assert!(
            !cs.last_click.contains_key(&chart),
            "last_click entry must be cleaned up"
        );
        assert!(
            !cs.dragged.contains(&chart),
            "dragged entry must be cleaned up"
        );
    }

    /// double-click reset は pan/zoom を既定へ戻し autoscale を再有効化する。
    #[test]
    fn reset_view_restores_autoscale_and_defaults() {
        use crate::ui::chart_viewstate::DEFAULT_CELL_WIDTH;
        let mut s = state_for_zoom();
        // pan/zoom 済みの状態を作る。
        s.translation = Vec2::new(40.0, -25.0);
        s.scaling = 2.5;
        s.cell_width = 22.0;
        s.auto_scale = false;
        s.reset_view();
        assert_eq!(s.translation, Vec2::ZERO);
        assert_eq!(s.scaling, 1.0);
        assert_eq!(s.cell_width, DEFAULT_CELL_WIDTH);
        assert!(s.auto_scale, "reset must re-enable autoscale");
    }

    /// y=0 のホイールイベント相当 (factor==1) は cell 寸法を変えず、translation も実質動かさない。
    /// translation.x は `x_to_time_ms` の ms 量子化で sub-pixel の round-trip 誤差を拾うため許容差で見る。
    #[test]
    fn zero_wheel_is_noop() {
        let mut s = state_for_zoom();
        let before = (s.cell_width, s.cell_height, s.translation);
        apply_cursor_zoom(&mut s, Vec2::new(10.0, 10.0), 0.0);
        assert_eq!(
            s.cell_width, before.0,
            "factor==1 must not change cell_width"
        );
        assert_eq!(
            s.cell_height, before.1,
            "factor==1 must not change cell_height"
        );
        assert!(
            (s.translation - before.2).length() < 1e-2,
            "factor==1 must leave translation effectively unchanged: {:?} vs {:?}",
            s.translation,
            before.2
        );
    }
}
