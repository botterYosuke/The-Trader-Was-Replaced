use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::trading::PortfolioState;

pub fn orders_panel_system(
    mut contexts: EguiContexts,
    portfolio: Option<Res<PortfolioState>>,
) {
    let Some(p) = portfolio else { return };

    egui::Window::new("Orders")
        .default_width(360.0)
        .collapsible(true)
        .resizable(true)
        .show(contexts.ctx_mut(), |ui| {
            if !p.loaded {
                ui.label(egui::RichText::new("No run yet").small().color(egui::Color32::GRAY));
                return;
            }
            if p.orders.is_empty() {
                ui.label(egui::RichText::new("No orders").small().color(egui::Color32::GRAY));
                return;
            }
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                egui::Grid::new("orders_grid")
                    .striped(true)
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        // Header
                        for h in &["Sym", "Side", "Qty", "Price", "Status"] {
                            ui.label(egui::RichText::new(*h).small().color(egui::Color32::from_rgb(0, 207, 255)));
                        }
                        ui.end_row();

                        for ord in &p.orders {
                            ui.label(egui::RichText::new(&ord.symbol).small().monospace());
                            let side_color = match ord.side.as_str() {
                                "BUY"  => egui::Color32::from_rgb(0, 255, 127),
                                "SELL" => egui::Color32::from_rgb(255, 51, 102),
                                _      => egui::Color32::GRAY,
                            };
                            ui.label(egui::RichText::new(&ord.side).small().monospace().color(side_color));
                            ui.label(egui::RichText::new(format!("{:.0}", ord.qty)).small().monospace());
                            ui.label(egui::RichText::new(format!("{:.0}", ord.price)).small().monospace());
                            ui.label(egui::RichText::new(&ord.status).small());
                            ui.end_row();
                        }
                    });
            });
        });
}
