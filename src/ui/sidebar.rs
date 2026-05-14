use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::trading::InstrumentList;

pub fn sidebar_system(
    mut contexts: EguiContexts,
    instrument_list: Option<Res<InstrumentList>>,
) {
    let Some(list) = instrument_list else { return };

    egui::Window::new("Instruments")
        .default_width(180.0)
        .default_height(300.0)
        .collapsible(true)
        .resizable(true)
        .show(contexts.ctx_mut(), |ui| {
            if !list.loaded {
                ui.label(egui::RichText::new("Loading…").small().color(egui::Color32::from_rgb(255, 200, 0)));
                return;
            }
            if let Some(err) = &list.error {
                ui.label(egui::RichText::new(format!("Error: {}", err)).small().color(egui::Color32::from_rgb(255, 51, 102)));
                return;
            }
            if list.ids.is_empty() {
                ui.label(egui::RichText::new("No instruments").small().color(egui::Color32::GRAY));
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for id in &list.ids {
                    ui.label(egui::RichText::new(id).small().monospace());
                }
            });
        });
}
