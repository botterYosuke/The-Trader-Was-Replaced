use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::ui::components::{StrategyBuffer, StrategyRunRequested};
use crate::trading::LastRunResult;

pub fn strategy_editor_window_system(
    mut contexts: EguiContexts,
    mut buffer: ResMut<StrategyBuffer>,
    mut run_events: EventWriter<StrategyRunRequested>,
    last_run: Option<Res<LastRunResult>>,
) {
    if buffer.original_path.is_none() {
        return;
    }

    let filename = buffer
        .original_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("<unnamed>");

    egui::Window::new(format!("Strategy: {}", filename))
        .default_width(800.0)
        .default_height(600.0)
        .show(contexts.ctx_mut(), |ui| {
            let can_save = buffer.cache_path.is_some() && buffer.dirty;
            let can_run = buffer.cache_path.is_some() && !buffer.dirty;
            let cache_path_clone = buffer.cache_path.clone();

            let mut save_clicked = false;
            let mut run_clicked = false;
            ui.horizontal(|ui| {
                save_clicked = ui.add_enabled(can_save, egui::Button::new("Save Cache")).clicked();
                run_clicked = ui.add_enabled(can_run, egui::Button::new("Run")).clicked();

                if let Some(path) = &cache_path_clone {
                    ui.label(format!(
                        "cache: {}",
                        path.file_name().and_then(|s| s.to_str()).unwrap_or("<cache>")
                    ));
                } else {
                    ui.label("cache: none");
                }
            });

            if save_clicked {
                if let Some(path) = cache_path_clone.clone() {
                    match std::fs::write(&path, &buffer.source) {
                        Ok(()) => {
                            buffer.dirty = false;
                            info!("strategy cache saved: {:?}", path);
                        }
                        Err(err) => {
                            error!("failed to save strategy cache {:?}: {}", path, err);
                        }
                    }
                }
            }

            if run_clicked {
                if let Some(path) = cache_path_clone {
                    run_events.send(StrategyRunRequested { cache_path: path });
                }
            }

            ui.separator();

            if let Some(run) = &last_run {
                if let Some(run_id) = &run.run_id {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Last run:").small().color(egui::Color32::from_rgb(0, 207, 255)));
                        ui.label(egui::RichText::new(run_id).small().monospace());
                    });
                    if let Some(s) = &run.parsed_summary {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("fills:").small());
                            ui.label(egui::RichText::new(s.fills_count.to_string()).small().monospace());
                            ui.label(egui::RichText::new("equity_pts:").small());
                            ui.label(egui::RichText::new(s.equity_points.to_string()).small().monospace());
                            ui.label(egui::RichText::new("total_pnl:").small());
                            let pnl_color = if s.total_pnl >= 0.0 {
                                egui::Color32::from_rgb(0, 255, 127)
                            } else {
                                egui::Color32::from_rgb(255, 51, 102)
                            };
                            ui.label(egui::RichText::new(format!("{:.0}", s.total_pnl)).small().monospace().color(pnl_color));
                            ui.label(egui::RichText::new("status:").small());
                            ui.label(egui::RichText::new(&s.status).small().monospace());
                        });
                    }
                    ui.separator();
                }
            }

            // Clone to avoid triggering Bevy change detection via DerefMut every frame.
            // Only write back (and mark changed) when egui reports actual content change.
            let mut source = buffer.source.clone();
            let response = ui.add(
                egui::TextEdit::multiline(&mut source)
                    .desired_width(f32::INFINITY)
                    .desired_rows(30),
            );

            if response.changed() {
                buffer.source = source;
                buffer.dirty = true;
            }
        });
}
