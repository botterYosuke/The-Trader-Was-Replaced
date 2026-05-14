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
