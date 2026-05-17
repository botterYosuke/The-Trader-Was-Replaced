use crate::ui::button::spawn_button;
use crate::ui::chart::ChartViewState;
use crate::ui::components::{ChartInstrument, PanelKind, PriceDisplay, TradeButton};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;

const PANEL_SIZE: Vec2 = Vec2::new(400.0, 500.0);
const PANEL_POSITION: Vec2 = Vec2::new(200.0, 0.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4);

pub fn spawn_chart_panel(commands: &mut Commands, instrument_id: &str) {
    // 枠は共通ヘルパーに任せる
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: format!("CHART — {}", instrument_id),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
        },
    );
    commands.entity(root).insert(PanelKind::Chart);
    commands.entity(root).insert(ChartInstrument {
        instrument_id: instrument_id.to_string(),
    });

    // ─── ここから下は中身（content_area の子として配置） ───

    // Price Display
    let price_text = commands
        .spawn((
            Text2d::new("$100.00"),
            TextFont {
                font_size: 60.0,
                ..default()
            },
            TextColor(Color::srgb(0.0, 1.0, 0.5)),
            Transform::from_xyz(0.0, 140.0, 0.1),
            PriceDisplay,
        ))
        .id();

    // Chart
    let chart = commands
        .spawn((
            Transform::from_xyz(0.0, 10.0, 0.1),
            ChartViewState {
                width: 360.0,
                height: 180.0,
                ..default()
            },
        ))
        .id();

    // Buttons
    let buy_button = spawn_button(
        commands,
        "BUY",
        Color::srgb(0.0, 0.8, 0.4),
        Vec2::new(-80.0, -160.0),
        TradeButton::Buy,
    );
    let sell_button = spawn_button(
        commands,
        "SELL",
        Color::srgb(0.8, 0.2, 0.2),
        Vec2::new(80.0, -160.0),
        TradeButton::Sell,
    );

    commands.entity(content_area).add_child(price_text);
    commands.entity(content_area).add_child(chart);
    commands.entity(content_area).add_child(buy_button);
    commands.entity(content_area).add_child(sell_button);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::{ChartInstrument, WindowRoot};

    #[test]
    fn spawn_chart_panel_attaches_chart_instrument_to_root() {
        let mut app = App::new();
        app.add_systems(Startup, |mut commands: Commands| {
            spawn_chart_panel(&mut commands, "1301.TSE");
        });
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let found: Vec<&ChartInstrument> = q.iter(world).collect();
        assert_eq!(found.len(), 1, "expected exactly 1 ChartInstrument on a WindowRoot");
        assert_eq!(found[0].instrument_id, "1301.TSE");
    }
}
