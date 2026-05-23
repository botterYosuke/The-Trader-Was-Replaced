use crate::trading::PortfolioState;
use crate::ui::components::PanelKind;
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;

// ── レイアウト & 配色 ─────────────────────────────────────────
const PANEL_SIZE: Vec2 = Vec2::new(280.0, 200.0);
const PANEL_POSITION: Vec2 = Vec2::new(-150.0, -270.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4);

const COLOR_HEADER: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_DEFAULT: Color = Color::srgb(0.85, 0.88, 0.94);
const COLOR_POS: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_NEG: Color = Color::srgb(1.0, 0.20, 0.40);
const COLOR_STATUS: Color = Color::srgb(0.55, 0.55, 0.55);

const MAX_ROWS: usize = 5;
const ROW_SPACING: f32 = 18.0;
const HEADER_Y: f32 = 45.0;
const ROW_0_Y: f32 = 27.0;

// ── 列定義 ───────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub enum PositionsColumn {
    Symbol,
    Qty,
    Avg,
    UPnl,
}

fn column_x(col: PositionsColumn) -> f32 {
    match col {
        PositionsColumn::Symbol => -100.0,
        PositionsColumn::Qty => -30.0,
        PositionsColumn::Avg => 40.0,
        PositionsColumn::UPnl => 100.0,
    }
}

// ── セル / ステータスマーカー ────────────────────────────────
#[derive(Component, Clone, Copy)]
pub struct PositionsCell {
    pub row: usize,
    pub col: PositionsColumn,
}

#[derive(Component)]
pub struct PositionsStatus;

// ── Spawn ────────────────────────────────────────────────────
pub fn spawn_positions_panel(commands: &mut Commands) {
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "POSITIONS".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
            closeable: true,
            resizable: false,
        },
    );
    commands.entity(root).insert(PanelKind::Positions);

    // ヘッダー行
    for (col, label) in [
        (PositionsColumn::Symbol, "Sym"),
        (PositionsColumn::Qty, "Qty"),
        (PositionsColumn::Avg, "Avg"),
        (PositionsColumn::UPnl, "uPnL"),
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

    // データセル（5 行 × 4 列、最初は空）
    for row in 0..MAX_ROWS {
        for col in [
            PositionsColumn::Symbol,
            PositionsColumn::Qty,
            PositionsColumn::Avg,
            PositionsColumn::UPnl,
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
                    PositionsCell { row, col },
                ))
                .id();
            commands.entity(content_area).add_child(cell);
        }
    }

    // ステータスメッセージ（"No run yet" / "No positions" を中央に表示）
    let status = commands
        .spawn((
            Text2d::new(""),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(COLOR_STATUS),
            Transform::from_xyz(0.0, -5.0, 0.15), // z 上げてセルより手前へ
            PositionsStatus,
        ))
        .id();
    commands.entity(content_area).add_child(status);
}

/// PortfolioState.positions を 5 行のテーブルに反映する。
/// 行数超過分は捨てる（MVP）。空・未ロード時は status テキストにメッセージ表示。
pub fn positions_panel_system(
    state: Res<PortfolioState>,
    mut cells: Query<(&PositionsCell, &mut Text2d, &mut TextColor)>,
    mut status_q: Query<&mut Text2d, (With<PositionsStatus>, Without<PositionsCell>)>,
) {
    // ─── status text の更新 ───
    let status_text = if !state.loaded {
        "No run yet"
    } else if state.positions.is_empty() {
        "No positions"
    } else {
        ""
    };
    if let Ok(mut t) = status_q.get_single_mut()
        && t.0 != status_text
    {
        t.0 = status_text.to_string();
    }

    // ─── データセルの更新 ───
    for (cell, mut text, mut color) in &mut cells {
        let (new_text, new_color) = if !state.loaded || cell.row >= state.positions.len() {
            (String::new(), COLOR_DEFAULT)
        } else {
            let p = &state.positions[cell.row];
            match cell.col {
                PositionsColumn::Symbol => (p.symbol.clone(), COLOR_DEFAULT),
                PositionsColumn::Qty => {
                    let c = if p.qty >= 0 { COLOR_POS } else { COLOR_NEG };
                    (p.qty.to_string(), c)
                }
                PositionsColumn::Avg => (format!("{:.0}", p.avg_price), COLOR_DEFAULT),
                PositionsColumn::UPnl => {
                    let c = if p.unrealized_pnl >= 0.0 {
                        COLOR_POS
                    } else {
                        COLOR_NEG
                    };
                    (format!("{:.0}", p.unrealized_pnl), c)
                }
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
