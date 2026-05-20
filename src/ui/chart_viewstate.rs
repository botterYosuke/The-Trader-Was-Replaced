//! Chart の座標系 / 表示状態 (`ChartViewState`) と autoscale パイプライン。
//!
//! flowsurface (`.claude/skills/flowsurface/src/src/chart.rs::ViewState`) の
//! `price_to_y` / `y_to_price` / `interval_to_x` / `x_to_interval` を Bevy に翻訳したもの。
//! iced の canvas は frame 変換 (translate/scale) を別レイヤで掛けるが、ShapePainter は
//! immediate-mode で座標を直接描くため、本実装では `translation` / `scaling` を座標ヘルパに
//! 直接畳み込む。Bevy の Y 軸は上が正 (iced は下が正) なので price→y は flowsurface の符号を反転。
//!
//! autoscale は self-`Changed` ループを避けるため `RequestAutoscale` イベントで分離する
//! (Caveat #25/#29): writer (`chart_data_tick_system`, map.is_changed gate) と
//! reader (`chart_interaction_tick_system`, `Changed<ChartViewState>` を read-only) と
//! consumer (`chart_autoscale_apply_system`, event 駆動 + DerefMut ガード) の 3 段。

use crate::trading::{InstrumentTradingData, InstrumentTradingDataMap, OhlcPoint};
use crate::ui::components::ChartInstrument;
// title bar 高さの single source (二重定義回避 — Caveat #33)。
use crate::ui::floating_window::TITLE_BAR_HEIGHT;
use bevy::prelude::*;

// ─── レイアウト定数 (Phase A で一括導入。Phase B の gutter / Phase F の Ladder が依存) ───

/// Y 軸ラベル領域 (chart の右、Phase B で使用)。
pub const PRICE_GUTTER_WIDTH: f32 = 50.0;
/// X 軸ラベル領域 (chart の下、Phase B で使用)。
pub const TIME_GUTTER_HEIGHT: f32 = 24.0;
/// 実描画領域 (Phase A から使用)。gutter 領域は含まない。
pub const CHART_DRAW_SIZE: Vec2 = Vec2::new(310.0, 180.0);
/// Replay モード (Phase A〜E) の WindowRoot サイズ。
///
/// ⚠️ 高さは draw(180) + time gutter(24) + title bar(40) = 244。Phase A の旧値 204 は
/// title bar を勘定しておらず、content 領域が 164px しか無く time gutter が窓外へ落ちた。
/// 幅は draw(310) + price gutter(50) = 360。chart child を左 `CHART_CHILD_LOCAL_X_REPLAY`・
/// 上 `CHART_CHILD_LOCAL_Y` だけ寄せて chart+gutter が枠内にちょうど収まる (window.rs spawn 参照)。
pub const CHART_PANEL_SIZE: Vec2 = Vec2::new(
    CHART_DRAW_SIZE.x + PRICE_GUTTER_WIDTH,                    // 360
    CHART_DRAW_SIZE.y + TIME_GUTTER_HEIGHT + TITLE_BAR_HEIGHT, // 244
);

/// Replay モードの chart child x オフセット (content_area-local)。
/// chart(310) を枠左端に flush し、右 50px を price gutter に空ける。
pub const CHART_CHILD_LOCAL_X_REPLAY: f32 = -PRICE_GUTTER_WIDTH / 2.0; // -25
/// chart child の y オフセット (content_area-local)。下 24px を time gutter に空けるため上へ寄せる。
pub const CHART_CHILD_LOCAL_Y: f32 = TIME_GUTTER_HEIGHT / 2.0; // +12

/// Ladder ペイン幅 (Phase F、Live モード複合ウィンドウ)。
pub const LADDER_WIDTH: f32 = 120.0;
/// Live モード WindowRoot サイズ。
pub const LIVE_COMBINED_PANEL_SIZE: Vec2 = Vec2::new(
    CHART_PANEL_SIZE.x + LADDER_WIDTH, // 480
    CHART_PANEL_SIZE.y,               // 244
);
/// Ladder ペインの WindowRoot ローカル X (center origin 前提: 幅 480 の右端 +240 に幅 120 を flush)。
pub const LADDER_PANE_LOCAL_X: f32 = LIVE_COMBINED_PANEL_SIZE.x / 2.0 - LADDER_WIDTH / 2.0; // 180
/// Live 時の chart child x オフセット (content_area-local)。
/// chart+gutter ブロック (360) を Live 枠 (480, center origin → [-240,240]) の左端へ flush する。
/// Replay の chart 中心 -25 から、枠が左へ `LADDER_WIDTH/2` (60) 広がった分だけ追従させて -85。
/// 結果: chart draw [-240,70] / price gutter [70,120] / Ladder [120,240] と枠 [-240,240] に収まる。
/// (旧値 -60 は Replay の -25 ベースを勘定せず、gutter が ladder と重なるバグだった)
pub const CHART_CHILD_LOCAL_X_LIVE: f32 = CHART_CHILD_LOCAL_X_REPLAY - LADDER_WIDTH / 2.0; // -85

/// 既定の 1 candle 幅 (px)。spawn 時と double-click reset (Phase E) の戻り先。
pub const DEFAULT_CELL_WIDTH: f32 = 6.0;

/// volume サブペインが draw 領域下部に占める割合 (Phase E で描画、Phase A では領域だけ予約)。
const VOLUME_AREA_RATIO: f32 = 0.2;
/// 最新 candle (`latest_x`) を draw 領域の右端からどれだけ内側に置くか (px)。
const RIGHT_EDGE_MARGIN: f32 = 8.0;

/// 集約基準。`Basis::Time(timeframe_ms)`。
#[derive(Debug, Clone, Copy)]
pub enum Basis {
    /// timeframe (ミリ秒)。例: 60_000 = 1 分足。
    Time(u64),
}

impl Basis {
    fn timeframe_ms(&self) -> f32 {
        match self {
            Basis::Time(ms) => *ms as f32,
        }
    }
}

/// 1 chart entity あたりの表示状態。pan/zoom/autoscale はすべてここに集約する。
///
/// ⚠️ `Default` は手書きする (Caveat #30)。`#[derive(Default)]` だと `auto_scale: false` で
/// spawn し chart が flat 表示になる。
#[derive(Component, Debug, Clone)]
pub struct ChartViewState {
    /// 描画矩形のサイズ (旧 width/height)。
    pub bounds: Vec2,
    /// pan オフセット (chart-local 座標)。
    pub translation: Vec2,
    /// ズーム倍率 (1.0 = 既定)。
    pub scaling: f32,
    /// 1 candle の幅 (px)。
    pub cell_width: f32,
    /// 1 tick の高さ (px)。autoscale が初回 frame で上書きする。
    pub cell_height: f32,
    /// 集約基準。
    pub basis: Basis,
    /// 最新 candle の open_time_ms (X 軸の右端基準)。
    pub latest_x: i64,
    /// `translation.y == 0` のとき main area 中央が指す価格。
    pub base_price_y: f32,
    /// 価格表示桁。
    pub decimals: usize,
    /// 最小価格単位 (axis label 刻み計算 / price↔y スケールの単位)。
    pub tick_size: f32,
    /// autoscale on/off。pan/zoom 開始で false になる。
    pub auto_scale: bool,
    /// per-instrument early-out 用 signature (末尾 bar を畳んだ u64)。
    pub last_seen_ohlc_signature: u64,
}

impl Default for ChartViewState {
    fn default() -> Self {
        Self {
            bounds: CHART_DRAW_SIZE,
            translation: Vec2::ZERO,
            scaling: 1.0,
            cell_width: DEFAULT_CELL_WIDTH,
            cell_height: 1.0, // autoscale が初回 frame で上書き
            basis: Basis::Time(60_000),
            latest_x: 0,
            base_price_y: 0.0,
            decimals: 2,
            tick_size: 0.01,
            auto_scale: true, // ⚠️ 必須: false だと flat 表示で spawn する
            last_seen_ohlc_signature: u64::MAX, // sentinel: 初回 data tick で必ず差分
        }
    }
}

impl ChartViewState {
    /// 集約 timeframe (ミリ秒)。time axis label の step テーブル選択に使う。
    pub fn timeframe_ms(&self) -> u64 {
        match self.basis {
            Basis::Time(ms) => ms,
        }
    }

    // ── レイアウト分割 (Phase A 先取り。Phase D crosshair gate / Phase E volume が依存) ──

    /// volume サブペインの高さ (draw 領域下部)。
    pub fn volume_area_height(&self) -> f32 {
        self.bounds.y * VOLUME_AREA_RATIO
    }

    /// main (price) area の高さ。
    pub fn main_area_height(&self) -> f32 {
        self.bounds.y - self.volume_area_height()
    }

    /// main area の下端 (= volume area の上端) の chart-local y。
    pub fn main_area_y_bottom(&self) -> f32 {
        -self.bounds.y / 2.0 + self.volume_area_height()
    }

    /// main area の中央 chart-local y。`base_price_y` がここに来る。
    fn main_area_center_y(&self) -> f32 {
        self.main_area_y_bottom() + self.main_area_height() / 2.0
    }

    /// 最新 candle を置く draw-local x (右端から `RIGHT_EDGE_MARGIN` 内側)。
    /// `translation.x == 0` のとき `interval_to_x(latest_x)` がこの値になる。
    fn right_anchor_x(&self) -> f32 {
        self.bounds.x / 2.0 - RIGHT_EDGE_MARGIN
    }

    /// 1 candle の body 半幅 (px)。最低 0.5px (細すぎて消えないように)。
    pub fn body_half_width(&self) -> f32 {
        (self.cell_width * self.scaling * 0.4).max(0.5)
    }

    /// pan/zoom をリセットして autoscale を再有効化する (double-click reset、Phase E)。
    ///
    /// `auto_scale = true` にすると次フレームの `chart_interaction_tick_system`
    /// (`Changed<ChartViewState>` reader) が `RequestAutoscale` を投げ、`base_price_y` /
    /// `cell_height` が再 fit される。translation / scaling / cell_width (時間軸ズーム) は
    /// autoscale が触らないのでここで既定へ戻す。`base_price_y` / `cell_height` は次フレームの
    /// autoscale が上書きするのでここでは触らない。
    pub fn reset_view(&mut self) {
        self.translation = Vec2::ZERO;
        self.scaling = 1.0;
        self.cell_width = DEFAULT_CELL_WIDTH;
        self.auto_scale = true;
    }

    // ── 座標変換 (flowsurface ViewState の翻訳。translation/scaling を畳み込み済み) ──

    /// 価格 → chart-local y。Bevy は上が正なので `price - base` (flowsurface は `base - price`)。
    pub fn price_to_y(&self, price: f32) -> f32 {
        let ticks = (price - self.base_price_y) / self.tick_size;
        self.main_area_center_y() + ticks * self.cell_height * self.scaling + self.translation.y
    }

    /// chart-local y → 価格 (`price_to_y` の逆関数)。
    pub fn y_to_price(&self, y: f32) -> f32 {
        let scaled = self.cell_height * self.scaling;
        let ticks = (y - self.main_area_center_y() - self.translation.y) / scaled;
        self.base_price_y + ticks * self.tick_size
    }

    /// candle 時刻 (open_time_ms) → chart-local x。`latest_x` が右端寄りに来る。
    pub fn interval_to_x(&self, open_time_ms: i64) -> f32 {
        let dt = (open_time_ms - self.latest_x) as f32;
        let cells = dt / self.basis.timeframe_ms();
        self.right_anchor_x() + cells * self.cell_width * self.scaling + self.translation.x
    }

    /// chart-local x → ms 時刻 (`interval_to_x` の逆関数)。
    pub fn x_to_time_ms(&self, x: f32) -> i64 {
        let scaled = self.cell_width * self.scaling;
        let cells = (x - self.right_anchor_x() - self.translation.x) / scaled;
        self.latest_x + (cells * self.basis.timeframe_ms()).round() as i64
    }

    /// 表示中の価格域 `(low, high)`。bounds 上下端を価格に逆写像。
    pub fn visible_price_range(&self) -> (f32, f32) {
        let top = self.bounds.y / 2.0;
        let bottom = -self.bounds.y / 2.0;
        let high = self.y_to_price(top);
        let low = self.y_to_price(bottom);
        (low.min(high), low.max(high))
    }

    /// main (price) area に表示中の価格域 `(low, high)`。volume area (下端 20%) を除く。
    /// 価格軸ラベルはこの範囲だけに引く (volume sub-pane の y 行に価格目盛りを出さない —
    /// crosshair の `hovered_price` が `main_area_y_bottom()` でガードしているのと対称)。
    pub fn main_area_price_range(&self) -> (f32, f32) {
        let high = self.y_to_price(self.bounds.y / 2.0);
        let low = self.y_to_price(self.main_area_y_bottom());
        (low.min(high), low.max(high))
    }

    /// 表示中の時刻域 `(earliest_ms, latest_ms)`。
    pub fn visible_time_range(&self) -> (i64, i64) {
        let left = -self.bounds.x / 2.0;
        let right = self.bounds.x / 2.0;
        let a = self.x_to_time_ms(left);
        let b = self.x_to_time_ms(right);
        (a.min(b), a.max(b))
    }

    /// draw 領域内に入る candle の連続スライス。autoscale (Phase A) と volume 集計 (Phase E) で共用。
    ///
    /// candle は open_time_ms 昇順、`interval_to_x` は単調増加なので可視区間は連続する。
    pub fn visible_candle_slice<'a>(&self, ohlc: &'a [OhlcPoint]) -> &'a [OhlcPoint] {
        if ohlc.is_empty() {
            return ohlc;
        }
        let margin = self.cell_width * self.scaling;
        let left = -self.bounds.x / 2.0 - margin;
        let right = self.bounds.x / 2.0 + margin;
        let start = ohlc
            .iter()
            .position(|pt| self.interval_to_x(pt.open_time_ms) >= left)
            .unwrap_or(ohlc.len());
        let end = ohlc
            .iter()
            .rposition(|pt| self.interval_to_x(pt.open_time_ms) <= right)
            .map(|i| i + 1)
            .unwrap_or(start);
        let start = start.min(ohlc.len());
        let end = end.clamp(start, ohlc.len());
        &ohlc[start..end]
    }
}

/// 末尾 bar の `(len, open_time_ms, high, low, close, volume)` を FNV-1a 風に畳んだ signature。
///
/// ⚠️ `DefaultHasher`/`AHasher::default()` は per-instance random key なので使わない (Caveat #14):
/// 同じ入力でも別の u64 を返し early-out が永久に成立しなくなる。決定的な自前 mix を使う。
/// `len()` 単独だと intra-bar の high/low/close 更新を取りこぼすため末尾 bar の値も混ぜる。
pub fn compute_ohlc_signature(ohlc: &[OhlcPoint]) -> u64 {
    const FNV_PRIME: u64 = 0x100000001b3;
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    let mut h: u64 = FNV_OFFSET;
    let mut mix = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(FNV_PRIME);
    };
    mix(ohlc.len() as u64);
    if let Some(last) = ohlc.last() {
        mix(last.open_time_ms as u64);
        mix(last.high.to_bits() as u64);
        mix(last.low.to_bits() as u64);
        mix(last.close.to_bits() as u64);
        mix(last.volume.unwrap_or(0.0).to_bits() as u64);
    }
    h
}

/// 可視 candle の high/low から `(base_price_y, cell_height)` を算出する (決定的)。
/// データが無ければ現状値を返す → DerefMut ガードと組んで no-op になる。
fn compute_autoscale(state: &ChartViewState, data: &InstrumentTradingData) -> (f32, f32) {
    let visible = state.visible_candle_slice(&data.ohlc_points);
    if visible.is_empty() {
        return (state.base_price_y, state.cell_height);
    }
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for pt in visible {
        if pt.high > max {
            max = pt.high;
        }
        if pt.low < min {
            min = pt.low;
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return (state.base_price_y, state.cell_height);
    }
    let raw = max - min;
    let (lo, hi) = if raw > 0.0 {
        let pad = raw * 0.1;
        (min - pad, max + pad)
    } else {
        (min - 1.0, max + 1.0)
    };
    let range = (hi - lo).max(f32::EPSILON);
    let base_price_y = (lo + hi) / 2.0;
    // main area の高さに padded range をちょうど収める cell_height。
    let cell_height = state.main_area_height() * state.tick_size / range;
    (base_price_y, cell_height)
}

// ─── システム実行順序 (Caveat #27) ───

/// chart 系 system の実行順を固定する set。observer (Pointer<Drag>/<Move>) は schedule 外なので
/// 含めない (Caveat #28)。
#[derive(SystemSet, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ChartSet {
    /// data tick + interaction tick (autoscale request の発火源)。
    DataTick,
    /// autoscale 適用 (base_price_y / cell_height 確定)。
    Autoscale,
    /// pan/zoom 等の regular interaction system (Phase C)。
    Interaction,
    /// 描画 system 群 (毎フレーム、ShapePainter)。
    Render,
}

/// autoscale 再計算リクエスト。data 経路と interaction 経路を分離するための内部イベント。
#[derive(Event, Debug, Clone, Copy)]
pub struct RequestAutoscale {
    pub chart: Entity,
}

/// map 変化を gate に latest_x を更新し、必要なら autoscale を要求する (純 writer)。
pub fn chart_data_tick_system(
    map: Res<InstrumentTradingDataMap>,
    mut chart_q: Query<(Entity, &ChartInstrument, &mut ChartViewState)>,
    mut req: EventWriter<RequestAutoscale>,
) {
    if !map.is_changed() {
        return;
    }
    for (e, ci, mut state) in &mut chart_q {
        let Some(data) = map.map.get(&ci.instrument_id) else {
            continue;
        };
        let signature = compute_ohlc_signature(&data.ohlc_points);
        if signature == state.last_seen_ohlc_signature {
            continue;
        }
        state.last_seen_ohlc_signature = signature;
        if let Some(last) = data.ohlc_points.last() {
            state.latest_x = last.open_time_ms;
        }
        if state.auto_scale {
            req.send(RequestAutoscale { chart: e });
        }
    }
}

/// pan/zoom 直後 + spawn フレームで autoscale を要求する (`Changed` を read-only に受ける)。
/// `Added<T>` は `Changed<T>` を含む (Caveat #32) ので spawn フレームの初回 autoscale も拾える。
pub fn chart_interaction_tick_system(
    interaction_q: Query<(Entity, &ChartViewState), Changed<ChartViewState>>,
    mut req: EventWriter<RequestAutoscale>,
) {
    for (e, state) in &interaction_q {
        if state.auto_scale {
            req.send(RequestAutoscale { chart: e });
        }
    }
}

/// `RequestAutoscale` を消費して `base_price_y` / `cell_height` を確定する (event 駆動)。
///
/// ⚠️ 収束の load-bearing 条件 (Caveat #29): 値が変化したときのみ `&mut state` 経由代入する。
/// 同値代入で DerefMut を踏むと `Changed` が立ち interaction_tick が再発火 → 無限 loop。
pub fn chart_autoscale_apply_system(
    mut events: EventReader<RequestAutoscale>,
    map: Res<InstrumentTradingDataMap>,
    mut chart_q: Query<(&ChartInstrument, &mut ChartViewState)>,
) {
    // dedupe: 同フレームに同 chart が複数回 request しても初出の 1 回だけ適用。
    let mut seen = std::collections::HashSet::<Entity>::new();
    for ev in events.read().filter(|ev| seen.insert(ev.chart)) {
        let Ok((ci, mut state)) = chart_q.get_mut(ev.chart) else {
            continue;
        };
        let Some(data) = map.map.get(&ci.instrument_id) else {
            continue;
        };
        let (new_base_price_y, new_cell_height) = compute_autoscale(&state, data);
        if (state.base_price_y - new_base_price_y).abs() > f32::EPSILON {
            state.base_price_y = new_base_price_y;
        }
        if (state.cell_height - new_cell_height).abs() > f32::EPSILON {
            state.cell_height = new_cell_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{InstrumentTradingData, OhlcPoint};

    fn ohlc(open_time_ms: i64, o: f32, h: f32, l: f32, c: f32) -> OhlcPoint {
        OhlcPoint {
            timestamp_ms: open_time_ms,
            open_time_ms,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: None,
        }
    }

    fn sample_data() -> InstrumentTradingData {
        let mut pts = Vec::new();
        // 60s 足を 10 本、close は 100 付近で振動。
        for i in 0..10i64 {
            let base = 100.0 + (i as f32);
            pts.push(ohlc(
                i * 60_000,
                base,
                base + 2.0,
                base - 2.0,
                base + 1.0,
            ));
        }
        InstrumentTradingData {
            ohlc_points: pts,
            ..Default::default()
        }
    }

    #[test]
    fn price_y_round_trip() {
        let mut state = ChartViewState::default();
        // autoscale 相当の現実的な値を入れる。
        state.base_price_y = 105.0;
        state.cell_height = 0.5;
        state.scaling = 1.3;
        state.translation = Vec2::new(7.0, -11.0);
        for &p in &[100.0_f32, 105.0, 110.5, 99.25, 123.456] {
            let y = state.price_to_y(p);
            let back = state.y_to_price(y);
            assert!(
                (back - p).abs() < 1e-2,
                "price round-trip failed: {} -> {} -> {}",
                p,
                y,
                back
            );
        }
    }

    #[test]
    fn time_x_round_trip() {
        let mut state = ChartViewState::default();
        state.latest_x = 600_000;
        state.cell_width = 6.0;
        state.scaling = 1.3;
        state.translation = Vec2::new(7.0, -11.0);
        for &t in &[0i64, 60_000, 540_000, 600_000, 300_000] {
            let x = state.interval_to_x(t);
            let back = state.x_to_time_ms(x);
            assert_eq!(back, t, "time round-trip failed: {} -> {} -> {}", t, x, back);
        }
    }

    #[test]
    fn autoscale_centers_data_in_main_area() {
        let data = sample_data();
        let mut state = ChartViewState::default();
        state.latest_x = data.ohlc_points.last().unwrap().open_time_ms;
        let (base, cell_h) = compute_autoscale(&state, &data);
        // base は可視 high/low の中点付近 (高値最大 ~111, 安値最小 ~98 → 中点 ~104.5)。
        assert!(base > 100.0 && base < 110.0, "base out of range: {}", base);
        assert!(cell_h > 0.0, "cell_height must be positive: {}", cell_h);

        // base に置いた状態で price_to_y(base) は main area 中央。
        let mut applied = state.clone();
        applied.base_price_y = base;
        applied.cell_height = cell_h;
        let y_center = applied.price_to_y(base);
        assert!(
            (y_center - applied.main_area_y_bottom() - applied.main_area_height() / 2.0).abs()
                < 1e-3
        );
    }

    /// Caveat #29: spawn → autoscale 適用後は `ChartViewState` が変化し続けないこと。
    #[test]
    fn autoscale_converges_within_few_frames() {
        #[derive(Resource, Default)]
        struct ChangedLog(Vec<usize>);

        let mut app = App::new();
        app.add_event::<RequestAutoscale>();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<ChangedLog>();

        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert("T".to_string(), sample_data());
        }
        app.world_mut().spawn((
            ChartViewState::default(),
            ChartInstrument {
                instrument_id: "T".to_string(),
            },
        ));

        app.add_systems(
            Update,
            (
                chart_data_tick_system,
                chart_interaction_tick_system,
                chart_autoscale_apply_system,
                |q: Query<(), Changed<ChartViewState>>, mut log: ResMut<ChangedLog>| {
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
        // frame 3 以降 (0-based index 2..) は変化が収束している。
        for (i, &c) in log.iter().enumerate() {
            if i >= 2 {
                assert_eq!(c, 0, "frame {} still mutating ChartViewState (log={:?})", i + 1, log);
            }
        }
    }

    #[test]
    fn signature_changes_on_intrabar_update() {
        let mut data = sample_data();
        let s1 = compute_ohlc_signature(&data.ohlc_points);
        // 末尾 bar の high を更新 (len は不変) → signature が変わる。
        let last = data.ohlc_points.last_mut().unwrap();
        last.high += 5.0;
        let s2 = compute_ohlc_signature(&data.ohlc_points);
        assert_ne!(s1, s2, "intra-bar high change must alter signature");
    }

    #[test]
    fn signature_is_deterministic() {
        let data = sample_data();
        let a = compute_ohlc_signature(&data.ohlc_points);
        let b = compute_ohlc_signature(&data.ohlc_points);
        assert_eq!(a, b, "signature must be deterministic across calls");
    }

    #[test]
    fn visible_slice_is_contiguous_subrange() {
        let data = sample_data();
        let mut state = ChartViewState::default();
        // chart_data_tick_system が設定する右端基準を再現する。
        state.latest_x = data.ohlc_points.last().unwrap().open_time_ms;
        let slice = state.visible_candle_slice(&data.ohlc_points);
        assert!(!slice.is_empty());
        // 最新 bar は必ず可視 (右端基準なので)。
        let last = data.ohlc_points.last().unwrap();
        assert_eq!(slice.last().unwrap().open_time_ms, last.open_time_ms);
    }
}
