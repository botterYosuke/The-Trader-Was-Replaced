//! Buying Power panel — Bevy world-space floating window.
//! Sub-step 1.3 で旧 egui::Window 実装から書き換え。

use crate::trading::PortfolioState;
use crate::ui::component::label::spawn_labeled_value_row;
use crate::ui::components::PanelKind;
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window_with_theme};
use crate::ui::theme::Theme;
use bevy::prelude::*;

const PANEL_SIZE: Vec2 = Vec2::new(270.0, 130.0);
const PANEL_POSITION: Vec2 = Vec2::new(-450.0, 100.0);

/// content_area 内の値テキスト行を識別するためのマーカー。
/// ラベル側（"equity:"）には貼らない。値側だけに貼って update system がここだけ書き換える。
#[derive(Component, Clone, Copy)]
pub enum BuyingPowerLabel {
    Equity,
    Cash,
    BuyingPower,
}

/// dispatcher から呼ばれる spawn 関数。
pub fn spawn_buying_power_panel(commands: &mut Commands, theme: &Theme) {
    let (root, content_area, _title_bar) = spawn_floating_window_with_theme(
        commands,
        FloatingWindowSpec {
            title: "BUYING POWER".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: theme.colors.accent.with_alpha(0.4),
            closeable: true,
            resizable: false,
        },
        theme,
    );
    // 重複防止用に PanelKind を root に貼る
    commands.entity(root).insert(PanelKind::BuyingPower);

    // 3 行: equity (top) / cash (mid) / buying_power (bottom)
    for (kind, label_text, y) in [
        (BuyingPowerLabel::Equity, "equity:", 20.0_f32),
        (BuyingPowerLabel::Cash, "cash:", 0.0),
        (BuyingPowerLabel::BuyingPower, "BP:", -20.0),
    ] {
        let (_label_e, value_e) = spawn_labeled_value_row(
            commands,
            content_area,
            label_text,
            "—",
            -100.0,
            60.0,
            y,
            theme,
        );
        // 値 entity にマーカーを付けて update system が書き換えられるようにする
        commands.entity(value_e).insert(kind);
    }
}

/// PortfolioState の現在値を 3 行のテキストに反映する。
pub fn buying_power_panel_system(
    state: Res<PortfolioState>,
    theme: Res<Theme>,
    mut q: Query<(&BuyingPowerLabel, &mut Text2d, &mut TextColor)>,
) {
    for (kind, mut text, mut color) in &mut q {
        if !state.loaded {
            if text.0 != "—" {
                text.0 = "—".to_string();
                color.0 = theme.colors.text;
            }
            continue;
        }
        let (value_str, value_color) = match kind {
            BuyingPowerLabel::Equity => {
                let v = state.equity;
                let c = if v >= 0.0 {
                    theme.status.success
                } else {
                    theme.status.error
                };
                (format!("{:.0}", v), c)
            }
            BuyingPowerLabel::Cash => (format!("{:.0}", state.cash), theme.colors.text),
            BuyingPowerLabel::BuyingPower => {
                (format!("{:.0}", state.buying_power), theme.colors.text)
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
