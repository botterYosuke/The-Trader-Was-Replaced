use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::trading::{LastRunResult, RunState};

pub fn run_result_panel_system(
    mut contexts: EguiContexts,
    last_run: Option<Res<LastRunResult>>,
) {
    let Some(run) = last_run else { return };

    egui::Window::new("Run Result")
        .default_width(240.0)
        .collapsible(true)
        .resizable(true)
        .show(contexts.ctx_mut(), |ui| {
            match &run.state {
                RunState::Idle => {
                    ui.label(egui::RichText::new("No run yet").small().color(egui::Color32::GRAY));
                }
                RunState::Running => {
                    ui.label(egui::RichText::new("Running…").small().color(egui::Color32::from_rgb(255, 200, 0)));
                }
                RunState::Completed => {
                    ui.label(egui::RichText::new("Completed").small().color(egui::Color32::from_rgb(0, 255, 127)));
                }
                RunState::Failed { error } => {
                    ui.label(egui::RichText::new(format!("Failed: {}", error)).small().color(egui::Color32::from_rgb(255, 51, 102)));
                }
            }

            if let Some(run_id) = &run.run_id {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("run:").small().color(egui::Color32::from_rgb(0, 207, 255)));
                    ui.label(egui::RichText::new(run_id).small().monospace());
                });
                if let Some(s) = &run.parsed_summary {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("fills:").small());
                        ui.label(egui::RichText::new(s.fills_count.to_string()).small().monospace());
                        ui.label(egui::RichText::new("eq_pts:").small());
                        ui.label(egui::RichText::new(s.equity_points.to_string()).small().monospace());
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("pnl:").small());
                        let pnl_color = if s.total_pnl >= 0.0 {
                            egui::Color32::from_rgb(0, 255, 127)
                        } else {
                            egui::Color32::from_rgb(255, 51, 102)
                        };
                        ui.label(egui::RichText::new(format!("{:.0}", s.total_pnl)).small().monospace().color(pnl_color));
                    });
                }
            }
        });
}
