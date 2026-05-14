//! Buying Power panel — Bevy world-space floating window.
//! Sub-step 1.3 で旧 egui::Window 実装から書き換え。

use crate::trading::PortfolioState;
use crate::ui::components::PanelKind;
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;

const PANEL_SIZE: Vec2 = Vec2::new(270.0, 130.0);
const PANEL_POSITION: Vec2 = Vec2::new(-450.0, 100.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4); // cyan rim

const COLOR_LABEL: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_VALUE_DEFAULT: Color = Color::srgb(0.85, 0.88, 0.94);
const COLOR_VALUE_POS: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_VALUE_NEG: Color = Color::srgb(1.0, 0.20, 0.40);

/// content_area 内の値テキスト行を識別するためのマーカー。
/// ラベル側（"equity:"）には貼らない。値側だけに貼って update system がここだけ書き換える。
#[derive(Component, Clone, Copy)]
pub enum BuyingPowerLabel {
    Equity,
    Cash,
    BuyingPower,
}

/// dispatcher から呼ばれる spawn 関数。
pub fn spawn_buying_power_panel(commands: &mut Commands) {
    let (root, content_area) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "BUYING POWER".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
        },
    );
    // 重複防止用に PanelKind を root に貼る
    commands.entity(root).insert(PanelKind::BuyingPower);

    // 3 行: equity (top) / cash (mid) / buying_power (bottom)
    spawn_row(
        commands,
        content_area,
        BuyingPowerLabel::Equity,
        20.0,
        "equity:",
    );
    spawn_row(commands, content_area, BuyingPowerLabel::Cash, 0.0, "cash:");
    spawn_row(
        commands,
        content_area,
        BuyingPowerLabel::BuyingPower,
        -20.0,
        "BP:",
    );
}

fn spawn_row(
    commands: &mut Commands,
    parent: Entity,
    kind: BuyingPowerLabel,
    y: f32,
    label_text: &str,
) {
    // ラベル（左寄せ、固定テキスト）
    let label = commands
        .spawn((
            Text2d::new(label_text),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(COLOR_LABEL),
            Transform::from_xyz(-100.0, y, 0.1),
        ))
        .id();
    commands.entity(parent).add_child(label);

    // 値（右側、update system が書き換える）
    let value = commands
        .spawn((
            Text2d::new("—"),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(COLOR_VALUE_DEFAULT),
            Transform::from_xyz(60.0, y, 0.1),
            kind, // ← マーカーとして PanelKind 同様に attach
        ))
        .id();
    commands.entity(parent).add_child(value);
}

/// PortfolioState の現在値を 3 行のテキストに反映する。
/// is_changed ゲートは付けない: 値ラベルがない（query 空）なら何もしないし、
/// 3 行のフォーマットは負荷的に毎フレームでも問題ない。
pub fn buying_power_panel_system(
    state: Res<PortfolioState>,
    mut q: Query<(&BuyingPowerLabel, &mut Text2d, &mut TextColor)>,
) {
    for (kind, mut text, mut color) in &mut q {
        if !state.loaded {
            if text.0 != "—" {
                text.0 = "—".to_string();
                color.0 = COLOR_VALUE_DEFAULT;
            }
            continue;
        }
        let (value_str, value_color) = match kind {
            BuyingPowerLabel::Equity => {
                let v = state.equity;
                let c = if v >= 0.0 {
                    COLOR_VALUE_POS
                } else {
                    COLOR_VALUE_NEG
                };
                (format!("{:.0}", v), c)
            }
            BuyingPowerLabel::Cash => (format!("{:.0}", state.cash), COLOR_VALUE_DEFAULT),
            BuyingPowerLabel::BuyingPower => {
                (format!("{:.0}", state.buying_power), COLOR_VALUE_DEFAULT)
            }
        };
        if text.0 != value_str {
            text.0 = value_str;
        }
        if color.0 != value_color {
            color.0 = value_color;
        }
    }
}
