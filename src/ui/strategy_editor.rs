use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::ui::components::StrategyBuffer;

pub fn strategy_editor_window_system(
    mut contexts: EguiContexts,
    mut buffer: ResMut<StrategyBuffer>,
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
            ui.horizontal(|ui| {
                let can_save = buffer.cache_path.is_some() && buffer.dirty;

                if ui.add_enabled(can_save, egui::Button::new("Save Cache")).clicked() {
                    if let Some(path) = buffer.cache_path.clone() {
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

                if let Some(path) = &buffer.cache_path {
                    ui.label(format!(
                        "cache: {}",
                        path.file_name().and_then(|s| s.to_str()).unwrap_or("<cache>")
                    ));
                } else {
                    ui.label("cache: none");
                }
            });

            ui.separator();

            let response = ui.add(
                egui::TextEdit::multiline(&mut buffer.source)
                    .desired_width(f32::INFINITY)
                    .desired_rows(30),
            );

            if response.changed() {
                buffer.dirty = true;
            }
        });
}
