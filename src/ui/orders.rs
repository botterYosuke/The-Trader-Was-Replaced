use crate::trading::{ExecutionModeRes, LiveOrders, PortfolioState, is_live_mode};
use crate::ui::components::PanelKind;
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use crate::ui::order_context_menu::OrderContextMenu;
use bevy::prelude::*;

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

const MAX_ROWS: usize = 6;
const ROW_SPACING: f32 = 18.0;
const HEADER_Y: f32 = 55.0;
const ROW_0_Y: f32 = 37.0;

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
                |down: Trigger<Pointer<Down>>,
                 hit_q: Query<&OrdersRowHit>,
                 live_orders: Res<LiveOrders>,
                 exec_mode: Res<ExecutionModeRes>,
                 venue: Res<crate::trading::VenueStatusRes>,
                 mut menu: ResMut<OrderContextMenu>| {
                    // Pointer<Down> は全ボタンで発火する → Secondary (右) のみ反応 (規約)。
                    if down.event().button != PointerButton::Secondary {
                        return;
                    }
                    // Live モードのみ。Replay 注文には取消/訂正を出さない。
                    if !is_live_mode(exec_mode.mode) {
                        return;
                    }
                    let Ok(hit) = hit_q.get(down.entity()) else {
                        return;
                    };
                    // その行に注文があるときだけ開く。
                    let Some(order) = live_orders.orders.get(hit.row) else {
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
pub fn orders_panel_system(
    state: Res<PortfolioState>,
    live_orders: Res<LiveOrders>,
    exec_mode: Res<ExecutionModeRes>,
    mut cells: Query<(&OrdersCell, &mut Text2d, &mut TextColor)>,
    mut status_q: Query<&mut Text2d, (With<OrdersStatus>, Without<OrdersCell>)>,
) {
    let live = is_live_mode(exec_mode.mode);
    let count = if live {
        live_orders.orders.len()
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

    // cells — 表示行のみソースから直接引く（中間 Vec を作らない）。
    for (cell, mut text, mut color) in &mut cells {
        let (new_text, new_color) = if cell.row >= count {
            (String::new(), COLOR_DEFAULT)
        } else if live {
            let o = &live_orders.orders[cell.row];
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
