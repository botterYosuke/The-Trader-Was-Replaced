use crate::trading::PortfolioState;
use crate::ui::components::PanelKind;
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
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

/// PortfolioState.orders を 6 行のテーブルに反映する。
/// 行数超過分は捨てる（MVP）。空・未ロード時は status テキストにメッセージ表示。
pub fn orders_panel_system(
    state: Res<PortfolioState>,
    mut cells: Query<(&OrdersCell, &mut Text2d, &mut TextColor)>,
    mut status_q: Query<&mut Text2d, (With<OrdersStatus>, Without<OrdersCell>)>,
) {
    // status text
    let status_text = if !state.loaded {
        "No run yet"
    } else if state.orders.is_empty() {
        "No orders"
    } else {
        ""
    };
    if let Ok(mut t) = status_q.get_single_mut() {
        if t.0 != status_text {
            t.0 = status_text.to_string();
        }
    }

    // cells
    for (cell, mut text, mut color) in &mut cells {
        let (new_text, new_color) = if !state.loaded || cell.row >= state.orders.len() {
            (String::new(), COLOR_DEFAULT)
        } else {
            let o = &state.orders[cell.row];
            match cell.col {
                OrdersColumn::Symbol => (o.symbol.clone(), COLOR_DEFAULT),
                OrdersColumn::Side => {
                    let c = match o.side.as_str() {
                        "BUY" => COLOR_BUY,
                        "SELL" => COLOR_SELL,
                        _ => COLOR_OTHER,
                    };
                    (o.side.clone(), c)
                }
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
