use crate::trading::{
    ExecutionModeRes, LiveOrders, OrdersFilter, PortfolioState, filter_label, is_live_mode,
    next_filter,
};
use crate::ui::components::PanelKind;
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use crate::ui::order_context_menu::OrderContextMenu;
use bevy::prelude::*;
use bevy::sprite::Anchor;

// ── レイアウト & 配色 ─────────────────────────────────────────
const PANEL_SIZE: Vec2 = Vec2::new(360.0, 220.0);
const PANEL_POSITION: Vec2 = Vec2::new(300.0, -270.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4);

const COLOR_HEADER: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_DEFAULT: Color = Color::srgb(0.85, 0.88, 0.94);
const COLOR_BUY: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_SELL: Color = Color::srgb(1.0, 0.20, 0.40);
const COLOR_OTHER: Color = Color::srgb(0.55, 0.55, 0.55);
const COLOR_STATUS: Color = Color::srgb(0.55, 0.55, 0.55);
const COLOR_FILTER: Color = Color::srgb(1.0, 0.78, 0.0);

const MAX_ROWS: usize = 6;
const ROW_SPACING: f32 = 18.0;
const HEADER_Y: f32 = 55.0;
const ROW_0_Y: f32 = 37.0;
const FILTER_Y: f32 = 78.0;
const FILTER_HIT_WIDTH: f32 = 200.0;

// ── 列定義 ───────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub enum OrdersColumn {
    Symbol,
    Side,
    Qty,
    Price,
    Status,
}

fn column_x(col: OrdersColumn) -> f32 {
    match col {
        OrdersColumn::Symbol => -130.0,
        OrdersColumn::Side => -60.0,
        OrdersColumn::Qty => 0.0,
        OrdersColumn::Price => 60.0,
        OrdersColumn::Status => 130.0,
    }
}

// ── セル / ステータスマーカー ────────────────────────────────
#[derive(Component, Clone, Copy)]
pub struct OrdersCell {
    pub row: usize,
    pub col: OrdersColumn,
}

#[derive(Component)]
pub struct OrdersStatus;

/// Phase 10 §2.9: the Text2d cell that shows the current OrdersFilter label
/// ("絞り込み: All" / "Manual" / "Strategy: …"). Live mode only.
#[derive(Component)]
pub struct OrdersFilterLabel;

/// Phase 10 §2.9: transparent clickable area over the filter label. A left-click
/// cycles `OrdersFilter` (All → Manual → each strategy → All). Same world-space
/// click-sprite flavor as `OrdersRowHit` (the panel is a Text2d world-space panel,
/// so per the bevy-engine skill the operable affordance is a pickable sprite +
/// Pointer<Pressed> observer, not a UI-node Button).
#[derive(Component)]
pub struct OrdersFilterHit;

/// Phase 9 §3.12: per-row transparent hit area for right-click → context menu.
/// `row` is the index into the displayed order list. 0.15 sprite picking is
/// bounds-based (alpha-agnostic), so a fully transparent sprite with a
/// `custom_size` is still pickable.
#[derive(Component, Clone, Copy)]
pub struct OrdersRowHit {
    pub row: usize,
}

const ROW_HIT_WIDTH: f32 = 320.0;

// ── Spawn ────────────────────────────────────────────────────
pub fn spawn_orders_panel(commands: &mut Commands) {
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "ORDERS".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
            closeable: true,
            resizable: false,
        },
    );
    commands.entity(root).insert(PanelKind::Orders);

    // ヘッダー行
    for (col, label) in [
        (OrdersColumn::Symbol, "Sym"),
        (OrdersColumn::Side, "Side"),
        (OrdersColumn::Qty, "Qty"),
        (OrdersColumn::Price, "Price"),
        (OrdersColumn::Status, "Status"),
    ] {
        let header = commands
            .spawn((
                Text2d::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(COLOR_HEADER),
                Transform::from_xyz(column_x(col), HEADER_Y, 0.1),
            ))
            .id();
        commands.entity(content_area).add_child(header);
    }

    // データセル（6 行 × 5 列）
    for row in 0..MAX_ROWS {
        for col in [
            OrdersColumn::Symbol,
            OrdersColumn::Side,
            OrdersColumn::Qty,
            OrdersColumn::Price,
            OrdersColumn::Status,
        ] {
            let y = ROW_0_Y - (row as f32) * ROW_SPACING;
            let cell = commands
                .spawn((
                    Text2d::new(""),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(COLOR_DEFAULT),
                    Transform::from_xyz(column_x(col), y, 0.1),
                    OrdersCell { row, col },
                ))
                .id();
            commands.entity(content_area).add_child(cell);
        }
    }

    // 各行の透明ヒット領域（右クリック → コンテキストメニュー）。Secondary ボタンのみ反応。
    // セルより僅かに背面 (z=0.05) に置き、行全幅をカバーする。
    for row in 0..MAX_ROWS {
        let y = ROW_0_Y - (row as f32) * ROW_SPACING;
        let hit = commands
            .spawn((
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.0),
                    custom_size: Some(Vec2::new(ROW_HIT_WIDTH, ROW_SPACING)),
                    ..default()
                },
                Transform::from_xyz(0.0, y, 0.05),
                OrdersRowHit { row },
            ))
            .observe(
                |down: Trigger<Pointer<Pressed>>,
                 hit_q: Query<&OrdersRowHit>,
                 live_orders: Res<LiveOrders>,
                 filter: Res<OrdersFilter>,
                 exec_mode: Res<ExecutionModeRes>,
                 venue: Res<crate::trading::VenueStatusRes>,
                 mut menu: ResMut<OrderContextMenu>| {
                    // Pointer<Pressed> は全ボタンで発火する → Secondary (右) のみ反応 (規約)。
                    if down.event().button != PointerButton::Secondary {
                        return;
                    }
                    // Live モードのみ。Replay 注文には取消/訂正を出さない。
                    if !is_live_mode(exec_mode.mode) {
                        return;
                    }
                    let Ok(hit) = hit_q.get(down.target()) else {
                        return;
                    };
                    // §2.9: index into the SAME filtered view the panel renders, so
                    // row N maps to the displayed order N (not the raw Vec index).
                    let Some(order) = live_orders.nth_filtered(&filter, hit.row) else {
                        return;
                    };
                    menu.open = true;
                    menu.client_order_id = Some(order.client_order_id.clone());
                    menu.venue = venue.venue_id.clone().unwrap_or_default();
                    menu.screen_pos = down.event().pointer_location.position;
                },
            )
            .id();
        commands.entity(content_area).add_child(hit);
    }

    // 絞り込みラベル (§2.9): Live モードのみ表示。クリックでフィルタを循環切替する。
    let filter_label = commands
        .spawn((
            Text2d::new(""),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(COLOR_FILTER),
            Anchor::CenterLeft,
            Transform::from_xyz(-150.0, FILTER_Y, 0.1),
            OrdersFilterLabel,
        ))
        .id();
    commands.entity(content_area).add_child(filter_label);

    // 絞り込みラベル上の透明クリック領域 (左クリックで循環)。
    let filter_hit = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.0, 0.0, 0.0, 0.0),
                custom_size: Some(Vec2::new(FILTER_HIT_WIDTH, ROW_SPACING)),
                ..default()
            },
            Transform::from_xyz(-150.0 + FILTER_HIT_WIDTH / 2.0, FILTER_Y, 0.05),
            OrdersFilterHit,
        ))
        .observe(
            |down: Trigger<Pointer<Pressed>>,
             exec_mode: Res<ExecutionModeRes>,
             live_orders: Res<LiveOrders>,
             mut filter: ResMut<OrdersFilter>| {
                // Pointer<Pressed> は全ボタンで発火する → Primary (左) のみ反応 (規約)。
                if down.event().button != PointerButton::Primary {
                    return;
                }
                // Live モードのみ。Replay 経路はフィルタを使わない。
                if !is_live_mode(exec_mode.mode) {
                    return;
                }
                let next = next_filter(&filter, &live_orders);
                if *filter != next {
                    *filter = next;
                }
            },
        )
        .id();
    commands.entity(content_area).add_child(filter_hit);

    // ステータスメッセージ
    let status = commands
        .spawn((
            Text2d::new(""),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(COLOR_STATUS),
            Transform::from_xyz(0.0, -5.0, 0.15),
            OrdersStatus,
        ))
        .id();
    commands.entity(content_area).add_child(status);
}

/// 1 セル分の (テキスト, 色)。side だけ色分けし、他は既定色。
fn side_color(side: &str) -> Color {
    match side {
        "BUY" => COLOR_BUY,
        "SELL" => COLOR_SELL,
        _ => COLOR_OTHER,
    }
}

/// 注文テーブルを 6 行に反映する。行数超過分は捨てる（MVP）。
///
/// Phase 9 §3.12: `ExecutionMode` が Live のときは UI が握る `LiveOrders`（発注 RPC 応答 +
/// `OrderEvent` push でマージされる）を、Replay のときは従来どおり `PortfolioState.orders` を
/// 表示する。Account/Position 同期は Step 4。
///
/// パネルは動的に再 spawn され得る（サイドバー toggle）ので毎フレーム回し、
/// 各セルは差分書き込み（規約 2）で no-op 時の change 発火を避ける。Vec 中間生成はせず、
/// 表示対象の ≤6 行だけソースから直接引く。
#[allow(clippy::type_complexity)]
pub fn orders_panel_system(
    state: Res<PortfolioState>,
    live_orders: Res<LiveOrders>,
    filter: Res<OrdersFilter>,
    exec_mode: Res<ExecutionModeRes>,
    mut cells: Query<(&OrdersCell, &mut Text2d, &mut TextColor)>,
    mut status_q: Query<
        &mut Text2d,
        (
            With<OrdersStatus>,
            Without<OrdersCell>,
            Without<OrdersFilterLabel>,
        ),
    >,
    mut filter_q: Query<
        &mut Text2d,
        (
            With<OrdersFilterLabel>,
            Without<OrdersCell>,
            Without<OrdersStatus>,
        ),
    >,
) {
    let live = is_live_mode(exec_mode.mode);
    // Live: pull the ≤6 displayed rows from the filtered view (§2.9) into a stack
    // array — no per-frame heap `Vec` (this system runs every frame). Replay:
    // PortfolioState (no filter). Both `nth_filtered` here and the right-click hit
    // observer use the same lookup, so row N maps to the same order in both.
    let mut live_view: [Option<&crate::trading::LiveOrder>; MAX_ROWS] = [None; MAX_ROWS];
    if live {
        for (row, slot) in live_view.iter_mut().enumerate() {
            *slot = live_orders.nth_filtered(&filter, row);
        }
    }
    let count = if live {
        live_view.iter().take_while(|o| o.is_some()).count()
    } else {
        state.orders.len()
    };

    // status text
    let status_text = if !live && !state.loaded {
        "No run yet"
    } else if count == 0 {
        "No orders"
    } else {
        ""
    };
    if let Ok(mut t) = status_q.get_single_mut()
        && t.0 != status_text
    {
        t.0 = status_text.to_string();
    }

    // 絞り込みラベル: Live のときのみ "Filter: <現在>"、Replay では空。
    // ラベルは ASCII のみ (OrdersPanel の他セルと同じ): Bevy 0.15 同梱の default_font
    // (FiraMono subset) は Basic Latin のみで CJK はトーフになるため (bevy-engine skill)。
    let filter_text = if live {
        format!("Filter: {}", filter_label(&filter))
    } else {
        String::new()
    };
    if let Ok(mut t) = filter_q.get_single_mut()
        && t.0 != filter_text
    {
        t.0 = filter_text;
    }

    // cells — 表示行のみソースから直接引く（中間 Vec を作らない）。
    for (cell, mut text, mut color) in &mut cells {
        let (new_text, new_color) = if cell.row >= count {
            (String::new(), COLOR_DEFAULT)
        } else if live {
            let o = live_view[cell.row].expect("row < count ⇒ slot is Some");
            match cell.col {
                OrdersColumn::Symbol => (o.symbol.clone(), COLOR_DEFAULT),
                OrdersColumn::Side => (o.side.clone(), side_color(&o.side)),
                OrdersColumn::Qty => (format!("{:.0}", o.qty), COLOR_DEFAULT),
                OrdersColumn::Price => match o.price {
                    Some(p) => (format!("{p:.0}"), COLOR_DEFAULT),
                    None => ("MKT".to_string(), COLOR_OTHER),
                },
                OrdersColumn::Status => (o.status.clone(), COLOR_DEFAULT),
            }
        } else {
            let o = &state.orders[cell.row];
            match cell.col {
                OrdersColumn::Symbol => (o.symbol.clone(), COLOR_DEFAULT),
                OrdersColumn::Side => (o.side.clone(), side_color(&o.side)),
                OrdersColumn::Qty => (format!("{:.0}", o.qty), COLOR_DEFAULT),
                OrdersColumn::Price => (format!("{:.0}", o.price), COLOR_DEFAULT),
                OrdersColumn::Status => (o.status.clone(), COLOR_DEFAULT),
            }
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
        if color.0 != new_color {
            color.0 = new_color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{ExecutionMode, LiveOrder};

    fn live_order(client_order_id: &str, symbol: &str, strategy_id: &str) -> LiveOrder {
        LiveOrder {
            client_order_id: client_order_id.to_string(),
            symbol: symbol.to_string(),
            side: "BUY".to_string(),
            qty: 100.0,
            price: Some(2500.0),
            status: "ACCEPTED".to_string(),
            strategy_id: strategy_id.to_string(),
            ..Default::default()
        }
    }

    /// Spawn the Symbol cell for each of `MAX_ROWS` rows and the filter label, run
    /// `orders_panel_system`, and read back the Symbol cells + filter label.
    fn run_panel(orders: Vec<LiveOrder>, filter: OrdersFilter) -> (Vec<String>, String) {
        let mut app = App::new();
        app.insert_resource(PortfolioState::default());
        let mut lo = LiveOrders::default();
        // upsert oldest-first so storage ends up newest-first like production.
        for o in orders.into_iter().rev() {
            lo.upsert_full(o);
        }
        app.insert_resource(lo);
        app.insert_resource(filter);
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveAuto,
        });
        app.add_systems(Update, orders_panel_system);

        let mut sym_cells = Vec::new();
        for row in 0..MAX_ROWS {
            let e = app
                .world_mut()
                .spawn((
                    Text2d::new(""),
                    TextColor(COLOR_DEFAULT),
                    OrdersCell {
                        row,
                        col: OrdersColumn::Symbol,
                    },
                ))
                .id();
            sym_cells.push(e);
        }
        let label = app
            .world_mut()
            .spawn((Text2d::new(""), OrdersFilterLabel))
            .id();
        app.update();

        let syms = sym_cells
            .iter()
            .map(|e| app.world().get::<Text2d>(*e).unwrap().0.clone())
            .collect();
        let label_text = app.world().get::<Text2d>(label).unwrap().0.clone();
        (syms, label_text)
    }

    #[test]
    fn live_panel_shows_all_when_filter_all() {
        let (syms, label) = run_panel(
            vec![
                live_order("c1", "7203.T", "MANUAL-001"),
                live_order("c2", "6758.T", "LIVE-abc"),
            ],
            OrdersFilter::All,
        );
        assert_eq!(syms[0], "7203.T");
        assert_eq!(syms[1], "6758.T");
        assert_eq!(syms[2], "", "no third order");
        assert_eq!(label, "Filter: All");
    }

    #[test]
    fn live_panel_narrows_to_manual() {
        let (syms, label) = run_panel(
            vec![
                live_order("c1", "7203.T", "MANUAL-001"),
                live_order("c2", "6758.T", "LIVE-abc"),
                live_order("c3", "9984.T", "MANUAL-001"),
            ],
            OrdersFilter::Manual,
        );
        // only the two MANUAL-001 orders, in storage order (c1=7203 then c3=9984;
        // c2=LIVE-abc filtered out).
        assert_eq!(syms[0], "7203.T");
        assert_eq!(syms[1], "9984.T");
        assert_eq!(syms[2], "", "LIVE-abc order is filtered out");
        assert_eq!(label, "Filter: Manual");
    }

    #[test]
    fn live_panel_narrows_to_specific_strategy() {
        let (syms, label) = run_panel(
            vec![
                live_order("c1", "7203.T", "MANUAL-001"),
                live_order("c2", "6758.T", "LIVE-abc"),
            ],
            OrdersFilter::Strategy("LIVE-abc".to_string()),
        );
        assert_eq!(syms[0], "6758.T");
        assert_eq!(syms[1], "", "manual order filtered out");
        // "LIVE-abc" is 8 chars → short_id leaves it intact.
        assert_eq!(label, "Filter: Strategy: LIVE-abc");
    }
}
