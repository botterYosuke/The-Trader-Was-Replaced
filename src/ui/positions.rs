use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::trading::PortfolioState;

pub fn positions_panel_system(
    mut contexts: EguiContexts,
    portfolio: Option<Res<PortfolioState>>,
) {
    let Some(p) = portfolio else { return };

    egui::Window::new("Positions")
        .default_width(280.0)
        .collapsible(true)
        .resizable(true)
        .show(contexts.ctx_mut(), |ui| {
            if !p.loaded {
                ui.label(egui::RichText::new("No run yet").small().color(egui::Color32::GRAY));
                return;
            }
            if p.positions.is_empty() {
                ui.label(egui::RichText::new("No positions").small().color(egui::Color32::GRAY));
                return;
            }
            egui::Grid::new("positions_grid")
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    // Header
                    for h in &["Sym", "Qty", "Avg", "uPnL"] {
                        ui.label(egui::RichText::new(*h).small().color(egui::Color32::from_rgb(0, 207, 255)));
                    }
                    ui.end_row();

                    for pos in &p.positions {
                        ui.label(egui::RichText::new(&pos.symbol).small().monospace());
                        let qty_color = if pos.qty >= 0 {
                            egui::Color32::from_rgb(0, 255, 127)
                        } else {
                            egui::Color32::from_rgb(255, 51, 102)
                        };
                        ui.label(egui::RichText::new(pos.qty.to_string()).small().monospace().color(qty_color));
                        ui.label(egui::RichText::new(format!("{:.0}", pos.avg_price)).small().monospace());
                        let upnl_color = if pos.unrealized_pnl >= 0.0 {
                            egui::Color32::from_rgb(0, 255, 127)
                        } else {
                            egui::Color32::from_rgb(255, 51, 102)
                        };
                        ui.label(egui::RichText::new(format!("{:.0}", pos.unrealized_pnl)).small().monospace().color(upnl_color));
                        ui.end_row();
                    }
                });
        });
}
