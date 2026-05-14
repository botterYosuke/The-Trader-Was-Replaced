use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::trading::PortfolioState;

pub fn buying_power_panel_system(
    mut contexts: EguiContexts,
    portfolio: Option<Res<PortfolioState>>,
) {
    let Some(p) = portfolio else { return };

    egui::Window::new("Buying Power")
        .default_width(200.0)
        .collapsible(true)
        .resizable(false)
        .show(contexts.ctx_mut(), |ui| {
            if !p.loaded {
                ui.label(egui::RichText::new("No run yet").small().color(egui::Color32::GRAY));
                return;
            }
            let label_color = egui::Color32::from_rgb(0, 207, 255);

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("equity:").small().color(label_color));
                let color = if p.equity >= 0.0 {
                    egui::Color32::from_rgb(0, 255, 127)
                } else {
                    egui::Color32::from_rgb(255, 51, 102)
                };
                ui.label(egui::RichText::new(format!("{:.0}", p.equity)).small().monospace().color(color));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("cash:").small().color(label_color));
                ui.label(egui::RichText::new(format!("{:.0}", p.cash)).small().monospace());
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("buying_power:").small().color(label_color));
                ui.label(egui::RichText::new(format!("{:.0}", p.buying_power)).small().monospace());
            });
        });
}
