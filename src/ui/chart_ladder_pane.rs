//! Live モード複合ウィンドウの Ladder ペイン (Phase 7.3 Phase F)。
//!
//! `ExecutionMode == LiveManual / LiveAuto` のとき、Chart ウィンドウの右側に bid/ask × 10 段と
//! LAST 行を結合した Ladder ペインを spawn する。`Replay` では despawn してコンパクトな
//! Chart のみのウィンドウに戻す (depth は Live 専用、Replay 表示禁止 — Phase 8 §0.5.1)。
//!
//! 設計 (flowsurface `data/src/panel/ladder.rs` の責務分割を Bevy に翻訳):
//! - **observer 不要**: Ladder は表示専用。ドラッグ伝播は WindowRoot 側の close button と同じく
//!   root 直下ではなく content_area の子に置くことで title bar drag observer に巻き込まれない。
//! - **mode_sync**: `ExecutionMode` 変化 *または* `Added<WindowRoot>` (Live 中に開いた chart) で
//!   Ladder の spawn/despawn + WindowRoot リサイズ + chart child の左シフトを reconcile する
//!   (Caveat #36)。
//! - **render**: per-instrument depth を `InstrumentTradingDataMap` から lookup し、行 (背景
//!   Sprite + Text2d) を `map.is_changed()` / `Added<LadderPane>` 駆動で despawn+respawn する
//!   retained 更新 (axis label と同じ流儀。ShapePainter は使わない — Caveat の z 衝突回避)。
//!
//! ⚠️ per-instrument を厳守する (Caveat: single-global `Res<TradingState>.depth` 退行を避ける)。
//! 各 `LadderPane` は自分の `chart_root` の `ChartInstrument.instrument_id` で depth を引く。

use crate::trading::{
    DepthLevel, DepthSnapshot, ExecutionModeRes, InstrumentTradingDataMap, LastPrices, is_live_mode,
};
use crate::ui::chart_render::{BEARISH_CANDLE_COLOR, BULLISH_CANDLE_COLOR};
use crate::ui::chart_viewstate::{
    CHART_CHILD_LOCAL_X_LIVE, CHART_CHILD_LOCAL_X_REPLAY, CHART_PANEL_SIZE, ChartViewState,
    LADDER_PANE_LOCAL_X, LADDER_WIDTH, LIVE_COMBINED_PANEL_SIZE,
};
use crate::ui::chart_volume::format_volume;
use crate::ui::components::{ChartInstrument, PriceDisplay, WindowRoot};
use crate::ui::floating_window::TITLE_BAR_HEIGHT;
use bevy::prelude::*;
use bevy::sprite::Anchor;

// ─── レイアウト定数 ───

/// Ladder ペインの縦サイズ。title bar を除いた content 領域に合わせる
/// (content_area は root-local (0, -title_bar_half) にあるので、ここを高さに使うと枠内に収まる)。
const LADDER_CONTENT_HEIGHT: f32 = CHART_PANEL_SIZE.y - TITLE_BAR_HEIGHT; // 204
/// 行数 = ask 10 + LAST 1 + bid 10 (Caveat #37: 常に 21 行固定)。
const LADDER_ROW_COUNT: f32 = 21.0;
/// Ladder の段数 (片側)。
const LADDER_DEPTH: usize = 10;

/// Ladder ペイン背景色。
const LADDER_PANE_BG: Color = Color::srgba(0.08, 0.08, 0.08, 0.95);
/// ペインの z (chart child +0.1 より前、crosshair badge +0.6 より後ろは問わない別 entity)。
const LADDER_PANE_Z: f32 = 0.2;
/// 行背景 Sprite の z (ペイン基準)。
const ROW_BG_Z: f32 = 0.05;
/// 行テキストの z (行背景の上)。
const ROW_TEXT_Z: f32 = 0.06;
/// 行テキストのフォントサイズ (px)。LADDER_WIDTH(120) に "12345.67 1.5K" が収まる小さめの値。
const ROW_TEXT_SIZE: f32 = 9.0;
/// 行テキストの左パディング。
const ROW_TEXT_PAD_X: f32 = 4.0;
/// bid/ask 行背景の薄塗り alpha (candle 色を流用 — Caveat: 色は const 一本化)。
const ROW_BG_ALPHA: f32 = 0.22;
/// LAST 行の背景色。
const LAST_ROW_BG: Color = Color::srgba(0.2, 0.2, 0.28, 0.95);
/// LAST 行のテキスト色。
const LAST_ROW_FG: Color = Color::srgb(1.0, 0.95, 0.55);
/// プレースホルダ (depth 無し) のテキスト色。
const PLACEHOLDER_FG: Color = Color::srgb(0.55, 0.55, 0.55);
/// price 表示桁 (ChartViewState の decimals と同じ hardcode 2)。
const LADDER_PRICE_DECIMALS: usize = 2;

// ─── Component ───

/// Ladder ペインが WindowRoot の Live 複合レイアウトとして生きていることを示すマーカー。
#[derive(Component)]
pub struct LadderPane {
    /// このペインが属する Chart `WindowRoot` (`ChartInstrument` を持つ)。
    pub chart_root: Entity,
    /// 直近に描いた depth + last の signature。`map.is_changed()` は他銘柄の OHLC tick でも
    /// 立つ粗い flag なので、自分の板が不変なら 21 行の despawn+respawn を skip する
    /// (chart_viewstate の `last_seen_ohlc_signature` と同じ per-entity early-out)。
    pub last_depth_signature: u64,
}

/// Ladder ペイン内の 1 行 (背景 Sprite + Text2d 子)。再生成時の despawn 対象判定に使う。
#[derive(Component, Clone, Copy)]
pub struct LadderRow {
    pub kind: LadderRowKind,
}

/// 行の種別。`Ask` (上半分) / `Last` (中央) / `Bid` (下半分)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LadderRowKind {
    Ask,
    Last,
    Bid,
}

// ─── 純関数 (テスト可能なレイアウト計算) ───

/// 1 行の高さ。行間隔 (`ladder_row_y`) と行背景 Sprite (`spawn_row`) で同一式を共有し desync を防ぐ。
fn ladder_row_height(ladder_height: f32) -> f32 {
    ladder_height / LADDER_ROW_COUNT
}

/// 種別 + index に対応する ladder-local y。中央 (LAST) を 0 とし、ask は上 (正)、bid は下 (負)。
/// best (`index == 0`) が中央に最も近く、worst (`index == 9`) が端になる (標準的な DOM ladder)。
pub fn ladder_row_y(kind: LadderRowKind, index: usize, ladder_height: f32) -> f32 {
    let row_height = ladder_row_height(ladder_height);
    match kind {
        LadderRowKind::Last => 0.0,
        LadderRowKind::Ask => (index as f32 + 1.0) * row_height,
        LadderRowKind::Bid => -(index as f32 + 1.0) * row_height,
    }
}

/// depth + last を u64 signature に畳む。値が不変なら行再生成を skip する early-out 用。
/// f64 は `to_bits()` で identity hash (NaN も bit パターンで一意)。
fn depth_signature(depth: Option<&DepthSnapshot>, last: Option<f64>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match depth {
        None => 0u8.hash(&mut h),
        Some(d) => {
            1u8.hash(&mut h);
            d.asks.len().hash(&mut h);
            d.bids.len().hash(&mut h);
            for lvl in d.asks.iter().chain(d.bids.iter()) {
                lvl.price.to_bits().hash(&mut h);
                lvl.size.to_bits().hash(&mut h);
            }
        }
    }
    match last {
        Some(p) => p.to_bits().hash(&mut h),
        None => u64::MAX.hash(&mut h),
    }
    h.finish()
}

/// bid/ask 行のラベル文字列。`None` (段数不足) は `---` プレースホルダ (Caveat #37: 常に 21 行)。
fn format_level_label(level: Option<&DepthLevel>) -> String {
    match level {
        Some(l) => format!(
            "{:.*}  {}",
            LADDER_PRICE_DECIMALS,
            l.price,
            format_volume(l.size as f32)
        ),
        None => "---".to_string(),
    }
}

// ─── システム: mode_sync (ExecutionMode 監視) ───

/// `ExecutionMode` 変化 / `Added<WindowRoot>` を監視し、Live 複合レイアウトを reconcile する。
///
/// Caveat #36: `exec_mode.is_changed() || !new_roots.is_empty()` で gate (不変フレームは早期 return)。
/// Caveat #40: chart child を Live で `CHART_CHILD_LOCAL_X_LIVE` (-85) へ左シフトしないと chart が
/// 中央に残り「左 chart・右 ladder」にならない。Caveat #34: 枠リサイズは `Sprite.custom_size` で。
pub fn chart_ladder_mode_sync_system(
    mut commands: Commands,
    exec_mode: Res<ExecutionModeRes>,
    // mode 不変でも Live 中に開いた chart root を拾うため Added を OR 条件に入れる。
    new_roots: Query<Entity, (With<WindowRoot>, With<ChartInstrument>, Added<WindowRoot>)>,
    chart_roots: Query<Entity, (With<WindowRoot>, With<ChartInstrument>)>,
    children_q: Query<&Children>,
    // chart child (content_area の孫) と price display を同量左シフトする。
    // ChartViewState と PriceDisplay は同居しないので Without で互いに disjoint。
    mut chart_tf: Query<&mut Transform, (With<ChartViewState>, Without<PriceDisplay>)>,
    mut price_tf: Query<&mut Transform, (With<PriceDisplay>, Without<ChartViewState>)>,
    ladder_panes: Query<(Entity, &LadderPane)>,
    mut root_sprites: Query<&mut Sprite, With<WindowRoot>>,
) {
    if !exec_mode.is_changed() && new_roots.is_empty() {
        return;
    }

    let is_live = is_live_mode(exec_mode.mode);
    let chart_x = if is_live {
        CHART_CHILD_LOCAL_X_LIVE
    } else {
        CHART_CHILD_LOCAL_X_REPLAY
    };

    for root_entity in &chart_roots {
        // root → content_area (chart/price を子に持つ root 直下 entity) を探し、その孫を左シフト。
        let mut content_area: Option<Entity> = None;
        if let Ok(root_children) = children_q.get(root_entity) {
            for rc in root_children.iter() {
                let Ok(grand) = children_q.get(rc) else {
                    continue;
                };
                let mut matched = false;
                // 実値が変わるときだけ書く (無条件代入は spurious な Changed<Transform> を立てる、
                // chart_viewstate.rs::chart_autoscale_apply_system と同じ DerefMut 抑制)。
                for g in grand.iter() {
                    if let Ok(mut tf) = chart_tf.get_mut(g) {
                        if tf.translation.x != chart_x {
                            tf.translation.x = chart_x;
                        }
                        matched = true;
                    } else if let Ok(mut tf) = price_tf.get_mut(g) {
                        if tf.translation.x != chart_x {
                            tf.translation.x = chart_x;
                        }
                        matched = true;
                    }
                }
                if matched {
                    content_area = Some(rc);
                    break;
                }
            }
        }

        // 枠サイズをモードに合わせて更新 (Caveat #34: custom_size のみ、Transform.scale は使わない)。
        // 実値が変わるときだけ書く (spurious な Changed<Sprite> を立てない)。
        if let Ok(mut sprite) = root_sprites.get_mut(root_entity) {
            let target = Some(if is_live {
                LIVE_COMBINED_PANEL_SIZE
            } else {
                CHART_PANEL_SIZE
            });
            if sprite.custom_size != target {
                sprite.custom_size = target;
            }
        }

        if is_live {
            let already_has_ladder = ladder_panes
                .iter()
                .any(|(_, lp)| lp.chart_root == root_entity);
            if !already_has_ladder {
                // content_area が見つかった root のみ (見つからない = teardown 中なら skip)。
                if let Some(content_area) = content_area {
                    let pane = commands
                        .spawn((
                            LadderPane {
                                chart_root: root_entity,
                                last_depth_signature: 0,
                            },
                            Sprite {
                                custom_size: Some(Vec2::new(LADDER_WIDTH, LADDER_CONTENT_HEIGHT)),
                                color: LADDER_PANE_BG,
                                ..default()
                            },
                            Transform::from_xyz(LADDER_PANE_LOCAL_X, 0.0, LADDER_PANE_Z),
                        ))
                        .id();
                    commands.entity(content_area).add_child(pane);
                }
            }
        } else {
            // Replay: この root に属する Ladder のみ despawn (子の行も含めて)。
            for (pane_entity, lp) in &ladder_panes {
                if lp.chart_root == root_entity {
                    commands.entity(pane_entity).despawn();
                }
            }
        }
    }
}

// ─── システム: render (per-instrument depth → 行生成) ───

/// per-instrument depth を読み、各 `LadderPane` の行 (背景 Sprite + Text2d) を再生成する。
///
/// Caveat: `map.is_changed()` (depth 更新) または `Added<LadderPane>` (新規ペイン) のフレームのみ
/// 再生成。`Ref<LadderPane>::is_added()` で「今フレーム spawn されたか」を判定する
/// (`Has<Added<T>>` は compile error — `Added` は filter であって Component ではない)。
pub fn ladder_render_system(
    mut commands: Commands,
    map: Res<InstrumentTradingDataMap>,
    last_prices: Res<LastPrices>,
    mut ladder_panes: Query<(Entity, &mut LadderPane, Option<&Children>)>,
    root_instruments: Query<&ChartInstrument>,
    rows_q: Query<(), With<LadderRow>>,
) {
    // depth は `InstrumentTradingDataMap`、LAST は `LastPrices` と別チャンネルで更新される
    // (main.rs の BackendStatusUpdate)。LAST だけ動いたフレームも拾わないと LAST 行が stale になる。
    let map_changed = map.is_changed() || last_prices.is_changed();
    for (pane_entity, mut lp, children) in &mut ladder_panes {
        let just_added = lp.is_added();
        if !map_changed && !just_added {
            continue;
        }

        // pane → root → instrument_id → depth / last。
        let Ok(ci) = root_instruments.get(lp.chart_root) else {
            continue;
        };
        let depth = map
            .map
            .get(&ci.instrument_id)
            .and_then(|d| d.depth.as_ref());
        let last = last_prices.map.get(&ci.instrument_id).copied();

        // 自分の板 (depth + last) が不変なら 21 行の再生成を skip (map.is_changed は他銘柄でも立つ)。
        let sig = depth_signature(depth, last);
        if !just_added && sig == lp.last_depth_signature {
            continue;
        }
        lp.last_depth_signature = sig;

        // この pane に属する古い行のみ despawn (行は Text2d 子を持つので recursive)。
        if let Some(children) = children {
            for child in children.iter() {
                if rows_q.contains(child) {
                    commands.entity(child).despawn();
                }
            }
        }

        match depth {
            Some(depth) => {
                // Ask 行 (上半分)。10 段未満は `---` で埋め常に 10 行 (Caveat #37)。
                for i in 0..LADDER_DEPTH {
                    spawn_ladder_row(
                        &mut commands,
                        pane_entity,
                        LadderRowKind::Ask,
                        i,
                        depth.asks.get(i),
                    );
                }
                // LAST 行 (中央)。last は per-instrument の LastPrices から引く (上で取得済)。
                spawn_ladder_last_row(&mut commands, pane_entity, last);
                // Bid 行 (下半分)。
                for i in 0..LADDER_DEPTH {
                    spawn_ladder_row(
                        &mut commands,
                        pane_entity,
                        LadderRowKind::Bid,
                        i,
                        depth.bids.get(i),
                    );
                }
            }
            None => {
                // depth 無し (Replay / 未購読): プレースホルダ 1 行。
                spawn_ladder_placeholder(&mut commands, pane_entity);
            }
        }
    }
}

// ─── 行 spawn ヘルパ ───

/// 行 entity (背景 Sprite + LadderRow) を spawn し、Text2d 子を付けて pane の子にする。
/// y / anchor / text_x は `kind` + `index` から導出する (LAST は中央寄せ、ask/bid は左寄せ)。
fn spawn_row(
    commands: &mut Commands,
    pane: Entity,
    kind: LadderRowKind,
    index: usize,
    bg: Color,
    text_color: Color,
    label: String,
) {
    let row_height = ladder_row_height(LADDER_CONTENT_HEIGHT);
    let y = ladder_row_y(kind, index, LADDER_CONTENT_HEIGHT);
    let (anchor, text_x) = match kind {
        LadderRowKind::Last => (Anchor::Center, 0.0),
        _ => (Anchor::CenterLeft, -LADDER_WIDTH / 2.0 + ROW_TEXT_PAD_X),
    };
    let row = commands
        .spawn((
            Sprite {
                custom_size: Some(Vec2::new(LADDER_WIDTH, row_height)),
                color: bg,
                ..default()
            },
            Transform::from_xyz(0.0, y, ROW_BG_Z),
            LadderRow { kind },
        ))
        .id();
    let text = commands
        .spawn((
            Text2d::new(label),
            TextFont {
                font_size: ROW_TEXT_SIZE,
                ..default()
            },
            TextColor(text_color),
            anchor,
            Transform::from_xyz(text_x, 0.0, ROW_TEXT_Z),
        ))
        .id();
    commands.entity(row).add_child(text);
    commands.entity(pane).add_child(row);
}

/// bid/ask 行を spawn する。`level == None` の段は `---`。
fn spawn_ladder_row(
    commands: &mut Commands,
    pane: Entity,
    kind: LadderRowKind,
    index: usize,
    level: Option<&DepthLevel>,
) {
    // この helper は Ask / Bid のみで呼ばれる (Last は spawn_ladder_last_row)。
    let base = if kind == LadderRowKind::Bid {
        BULLISH_CANDLE_COLOR
    } else {
        BEARISH_CANDLE_COLOR
    };
    spawn_row(
        commands,
        pane,
        kind,
        index,
        base.with_alpha(ROW_BG_ALPHA),
        base,
        format_level_label(level),
    );
}

/// LAST 行 (中央) を spawn する。`last == None` なら価格を `---` 表示。
fn spawn_ladder_last_row(commands: &mut Commands, pane: Entity, last: Option<f64>) {
    let label = match last {
        Some(p) => format!("LAST {:.*}", LADDER_PRICE_DECIMALS, p),
        None => "LAST ---".to_string(),
    };
    spawn_row(
        commands,
        pane,
        LadderRowKind::Last,
        0,
        LAST_ROW_BG,
        LAST_ROW_FG,
        label,
    );
}

/// depth が無い (Replay / 未購読) ときのプレースホルダ行。再生成時に他行と同様 despawn される。
/// 行ヘルパを再利用し、背景のみ透明にして中央テキストだけ見せる。
fn spawn_ladder_placeholder(commands: &mut Commands, pane: Entity) {
    spawn_row(
        commands,
        pane,
        LadderRowKind::Last,
        0,
        Color::NONE,
        PLACEHOLDER_FG,
        // ⚠️ ASCII 限定: 既定フォント (FiraMono-subset) は CJK グリフを持たず日本語は豆腐になる
        //    (codebase 全体で UI 文字列は ASCII。buying_power の "—" 等が前例)。
        "No depth data".to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{DepthSnapshot, ExecutionMode, ExecutionModeRes, InstrumentTradingData};
    use crate::ui::chart_viewstate::{CHART_DRAW_SIZE, PRICE_GUTTER_WIDTH};

    fn level(price: f64, size: f64) -> DepthLevel {
        DepthLevel { price, size }
    }

    fn ten_levels() -> Vec<DepthLevel> {
        (0..10)
            .map(|i| level(100.0 + i as f64, 10.0 * (i + 1) as f64))
            .collect()
    }

    /// ask は中央より上、bid は下、best (index 0) が中央に最も近い。
    #[test]
    fn ladder_row_y_orders_asks_above_bids() {
        let h = LADDER_CONTENT_HEIGHT;
        let last = ladder_row_y(LadderRowKind::Last, 0, h);
        assert_eq!(last, 0.0);

        let best_ask = ladder_row_y(LadderRowKind::Ask, 0, h);
        let worst_ask = ladder_row_y(LadderRowKind::Ask, 9, h);
        assert!(best_ask > 0.0, "ask は中央より上 (正)");
        assert!(worst_ask > best_ask, "worst ask (index 9) が best より上");

        let best_bid = ladder_row_y(LadderRowKind::Bid, 0, h);
        let worst_bid = ladder_row_y(LadderRowKind::Bid, 9, h);
        assert!(best_bid < 0.0, "bid は中央より下 (負)");
        assert!(worst_bid < best_bid, "worst bid (index 9) が best より下");

        // 全 21 行が ladder 高さ内に収まる (端の行中心が ±height/2 を超えない)。
        assert!(
            worst_ask <= h / 2.0 + 1e-3,
            "worst ask {worst_ask} > {}",
            h / 2.0
        );
        assert!(
            worst_bid >= -h / 2.0 - 1e-3,
            "worst bid {worst_bid} < {}",
            -h / 2.0
        );
    }

    /// Live レイアウト: chart draw / price gutter / Ladder が枠 [-240,240] に隙間なく収まる。
    /// CHART_CHILD_LOCAL_X_LIVE が -60 のままだと gutter が ladder と重なりこのテストが落ちる。
    #[test]
    fn live_layout_fits_frame_without_overlap() {
        let frame_half = LIVE_COMBINED_PANEL_SIZE.x / 2.0; // 240
        let chart_half = CHART_DRAW_SIZE.x / 2.0; // 155

        let chart_left = CHART_CHILD_LOCAL_X_LIVE - chart_half;
        let chart_right = CHART_CHILD_LOCAL_X_LIVE + chart_half;
        // price gutter は chart-local +180 (= chart_half + PRICE_GUTTER_WIDTH/2)。
        let gutter_center = CHART_CHILD_LOCAL_X_LIVE + chart_half + PRICE_GUTTER_WIDTH / 2.0;
        let gutter_left = gutter_center - PRICE_GUTTER_WIDTH / 2.0;
        let gutter_right = gutter_center + PRICE_GUTTER_WIDTH / 2.0;
        let ladder_left = LADDER_PANE_LOCAL_X - LADDER_WIDTH / 2.0;
        let ladder_right = LADDER_PANE_LOCAL_X + LADDER_WIDTH / 2.0;

        assert!(
            (chart_left + frame_half).abs() < 1e-3,
            "chart 左端 {chart_left} が枠左端 {} に flush",
            -frame_half
        );
        assert!(
            (gutter_left - chart_right).abs() < 1e-3,
            "gutter は chart の右に隣接"
        );
        assert!(
            (ladder_left - gutter_right).abs() < 1e-3,
            "ladder は gutter の右に隣接 (重なり無し)"
        );
        assert!(
            (ladder_right - frame_half).abs() < 1e-3,
            "ladder 右端 {ladder_right} が枠右端 {frame_half} に flush"
        );
    }

    /// content_area の孫 (chart/price) を持つ最小階層を組み、mode_sync の spawn/despawn を検証。
    fn build_min_chart(app: &mut App, instrument: &str) -> (Entity, Entity, Entity) {
        let root = app
            .world_mut()
            .spawn((
                WindowRoot,
                ChartInstrument {
                    instrument_id: instrument.to_string(),
                },
                Sprite {
                    custom_size: Some(CHART_PANEL_SIZE),
                    ..default()
                },
                Transform::default(),
            ))
            .id();
        let content_area = app.world_mut().spawn(Transform::default()).id();
        let chart = app
            .world_mut()
            .spawn((
                ChartViewState::default(),
                ChartInstrument {
                    instrument_id: instrument.to_string(),
                },
                Transform::from_xyz(CHART_CHILD_LOCAL_X_REPLAY, 0.0, 0.1),
            ))
            .id();
        let price = app
            .world_mut()
            .spawn((
                PriceDisplay,
                Transform::from_xyz(CHART_CHILD_LOCAL_X_REPLAY, 0.0, 0.3),
            ))
            .id();
        app.world_mut()
            .entity_mut(content_area)
            .add_child(chart)
            .add_child(price);
        app.world_mut().entity_mut(root).add_child(content_area);
        (root, chart, price)
    }

    #[test]
    fn mode_sync_spawns_ladder_in_live_and_despawns_in_replay() {
        let mut app = App::new();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.add_systems(Update, chart_ladder_mode_sync_system);

        let (root, chart, price) = build_min_chart(&mut app, "X");

        // Live: ladder spawn + 枠 Live サイズ + chart/price 左シフト。
        app.update();
        {
            let world = app.world_mut();
            let mut pq = world.query::<&LadderPane>();
            let panes: Vec<_> = pq.iter(world).collect();
            assert_eq!(panes.len(), 1, "Live で Ladder が 1 つ spawn される");
            assert_eq!(panes[0].chart_root, root);

            let sprite = world.entity(root).get::<Sprite>().unwrap();
            assert_eq!(sprite.custom_size, Some(LIVE_COMBINED_PANEL_SIZE));

            let chart_x = world
                .entity(chart)
                .get::<Transform>()
                .unwrap()
                .translation
                .x;
            let price_x = world
                .entity(price)
                .get::<Transform>()
                .unwrap()
                .translation
                .x;
            assert!(
                (chart_x - CHART_CHILD_LOCAL_X_LIVE).abs() < 1e-3,
                "chart 左シフト"
            );
            assert!(
                (price_x - CHART_CHILD_LOCAL_X_LIVE).abs() < 1e-3,
                "price 左シフト"
            );
        }

        // Replay に戻す: ladder despawn + 枠 Replay サイズ + chart/price 復帰。
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
        app.update();
        {
            let world = app.world_mut();
            let mut pq = world.query::<&LadderPane>();
            assert_eq!(
                pq.iter(world).count(),
                0,
                "Replay で Ladder が despawn される"
            );

            let sprite = world.entity(root).get::<Sprite>().unwrap();
            assert_eq!(sprite.custom_size, Some(CHART_PANEL_SIZE));

            let chart_x = world
                .entity(chart)
                .get::<Transform>()
                .unwrap()
                .translation
                .x;
            assert!(
                (chart_x - CHART_CHILD_LOCAL_X_REPLAY).abs() < 1e-3,
                "chart x 復帰"
            );
        }
    }

    /// depth がある pane は 21 行 (ask10 + last + bid10) を生成する。
    #[test]
    fn ladder_render_builds_21_rows_from_depth() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<LastPrices>();
        app.add_systems(Update, ladder_render_system);

        let root = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "X".to_string(),
            })
            .id();
        let pane = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();

        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: Some(DepthSnapshot {
                        bids: ten_levels(),
                        asks: ten_levels(),
                        timestamp_ms: None,
                    }),
                    ..default()
                },
            );
        }
        app.world_mut()
            .resource_mut::<LastPrices>()
            .map
            .insert("X".to_string(), 105.0);

        app.update();

        let world = app.world_mut();
        let mut rq = world.query::<(&LadderRow, &ChildOf)>();
        let rows: Vec<_> = rq.iter(world).filter(|(_, p)| p.get() == pane).collect();
        assert_eq!(rows.len(), 21, "ask10 + last + bid10 = 21 行");
        assert_eq!(
            rows.iter()
                .filter(|(r, _)| r.kind == LadderRowKind::Ask)
                .count(),
            10
        );
        assert_eq!(
            rows.iter()
                .filter(|(r, _)| r.kind == LadderRowKind::Bid)
                .count(),
            10
        );
        assert_eq!(
            rows.iter()
                .filter(|(r, _)| r.kind == LadderRowKind::Last)
                .count(),
            1
        );
    }

    /// depth が None の pane はプレースホルダ 1 行のみ。
    #[test]
    fn ladder_render_placeholder_when_no_depth() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<LastPrices>();
        app.add_systems(Update, ladder_render_system);

        let root = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "X".to_string(),
            })
            .id();
        let pane = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();
        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: None,
                    ..default()
                },
            );
        }

        app.update();

        let world = app.world_mut();
        let mut rq = world.query::<(&LadderRow, &ChildOf)>();
        let rows: Vec<_> = rq.iter(world).filter(|(_, p)| p.get() == pane).collect();
        assert_eq!(rows.len(), 1, "depth 無しはプレースホルダ 1 行");
    }

    /// 複数銘柄: depth を持つ銘柄の pane のみ板が出る (single-global 退行が無い)。
    #[test]
    fn ladder_render_is_per_instrument() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<LastPrices>();
        app.add_systems(Update, ladder_render_system);

        let root_x = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "X".to_string(),
            })
            .id();
        let root_y = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "Y".to_string(),
            })
            .id();
        let pane_x = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root_x,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();
        let pane_y = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root_y,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();

        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            // X だけ depth あり、Y は depth 無し。
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: Some(DepthSnapshot {
                        bids: ten_levels(),
                        asks: ten_levels(),
                        timestamp_ms: None,
                    }),
                    ..default()
                },
            );
            map.map.insert(
                "Y".to_string(),
                InstrumentTradingData {
                    depth: None,
                    ..default()
                },
            );
        }

        app.update();

        let world = app.world_mut();
        let mut rq = world.query::<(&LadderRow, &ChildOf)>();
        let all: Vec<_> = rq.iter(world).collect();
        let x_rows = all.iter().filter(|(_, p)| p.get() == pane_x).count();
        let y_rows = all.iter().filter(|(_, p)| p.get() == pane_y).count();
        assert_eq!(x_rows, 21, "depth ありの X は 21 行");
        assert_eq!(
            y_rows, 1,
            "depth 無しの Y はプレースホルダ 1 行 (X の板を共有しない)"
        );
    }

    /// depth が変化したフレームで行が累積せず 21 行に置き換わる (signature guard 経由の rebuild)。
    #[test]
    fn ladder_rows_replace_not_accumulate() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<LastPrices>();
        app.add_systems(Update, ladder_render_system);

        let root = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "X".to_string(),
            })
            .id();
        let pane = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();
        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: Some(DepthSnapshot {
                        bids: ten_levels(),
                        asks: ten_levels(),
                        timestamp_ms: None,
                    }),
                    ..default()
                },
            );
        }

        app.update();
        // depth を別の値に差し替える (signature が変わり rebuild される)。
        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: Some(DepthSnapshot {
                        bids: (0..10).map(|i| level(200.0 + i as f64, 5.0)).collect(),
                        asks: (0..10).map(|i| level(210.0 + i as f64, 5.0)).collect(),
                        timestamp_ms: None,
                    }),
                    ..default()
                },
            );
        }
        app.update();

        let world = app.world_mut();
        let mut rq = world.query::<(&LadderRow, &ChildOf)>();
        let rows = rq.iter(world).filter(|(_, p)| p.get() == pane).count();
        assert_eq!(rows, 21, "depth 変化で再生成後も 21 行 (42 に累積しない)");
    }

    /// 自分の板が不変なら map.is_changed() が立っても行を再生成しない (signature early-out)。
    #[test]
    fn ladder_skips_rebuild_when_depth_unchanged() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<LastPrices>();
        app.add_systems(Update, ladder_render_system);

        let root = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "X".to_string(),
            })
            .id();
        let pane = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();
        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: Some(DepthSnapshot {
                        bids: ten_levels(),
                        asks: ten_levels(),
                        timestamp_ms: None,
                    }),
                    ..default()
                },
            );
        }

        app.update();
        // 初回 build で行 entity の id を控える。
        let first_ids: Vec<Entity> = {
            let world = app.world_mut();
            let mut rq = world.query::<(Entity, &LadderRow, &ChildOf)>();
            rq.iter(world)
                .filter(|(_, _, p)| p.get() == pane)
                .map(|(e, _, _)| e)
                .collect()
        };
        assert_eq!(first_ids.len(), 21);

        // depth は同値のまま map.is_changed() だけ立てる → 再生成されず同じ entity が残る。
        app.world_mut()
            .resource_mut::<InstrumentTradingDataMap>()
            .set_changed();
        app.update();

        let world = app.world_mut();
        let mut rq = world.query::<(Entity, &LadderRow, &ChildOf)>();
        let second_ids: Vec<Entity> = rq
            .iter(world)
            .filter(|(_, _, p)| p.get() == pane)
            .map(|(e, _, _)| e)
            .collect();
        assert_eq!(
            second_ids, first_ids,
            "depth 不変なら同じ行 entity が残る (rebuild skip)"
        );
    }

    /// LAST だけ更新されたフレーム (depth 不変、map は無変更) でも LAST 行が rebuild される。
    /// gate が `map.is_changed()` 単独だと LAST が stale になる回帰を防ぐ (efficiency M2)。
    #[test]
    fn ladder_rebuilds_on_last_price_only_change() {
        let mut app = App::new();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<LastPrices>();
        app.add_systems(Update, ladder_render_system);

        let root = app
            .world_mut()
            .spawn(ChartInstrument {
                instrument_id: "X".to_string(),
            })
            .id();
        let pane = app
            .world_mut()
            .spawn((
                LadderPane {
                    chart_root: root,
                    last_depth_signature: 0,
                },
                Transform::default(),
            ))
            .id();
        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map.insert(
                "X".to_string(),
                InstrumentTradingData {
                    depth: Some(DepthSnapshot {
                        bids: ten_levels(),
                        asks: ten_levels(),
                        timestamp_ms: None,
                    }),
                    ..default()
                },
            );
        }
        app.world_mut()
            .resource_mut::<LastPrices>()
            .map
            .insert("X".to_string(), 100.0);
        app.update();
        let first_ids: Vec<Entity> = {
            let world = app.world_mut();
            let mut rq = world.query::<(Entity, &LadderRow, &ChildOf)>();
            rq.iter(world)
                .filter(|(_, _, p)| p.get() == pane)
                .map(|(e, _, _)| e)
                .collect()
        };
        assert_eq!(first_ids.len(), 21);

        // map は触らず LAST だけ変える (LastPrices.is_changed() のみ立つ)。
        app.world_mut()
            .resource_mut::<LastPrices>()
            .map
            .insert("X".to_string(), 999.0);
        app.update();

        let world = app.world_mut();
        let mut rq = world.query::<(Entity, &LadderRow, &ChildOf)>();
        let second_ids: Vec<Entity> = rq
            .iter(world)
            .filter(|(_, _, p)| p.get() == pane)
            .map(|(e, _, _)| e)
            .collect();
        assert_eq!(second_ids.len(), 21);
        assert_ne!(
            second_ids, first_ids,
            "LAST 変化で rebuild され行 entity が入れ替わる (depth 不変でも反映)"
        );
    }
}
