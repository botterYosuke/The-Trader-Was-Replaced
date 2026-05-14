use crate::ui::components::{PanelKind, StrategyBuffer, StrategyRunRequested};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;
use bevy_cosmic_edit::CursorColor;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Metrics};
use bevy_cosmic_edit::prelude::*;
use bevy_egui::{EguiContexts, egui};

pub fn strategy_editor_window_system(
    mut contexts: EguiContexts,
    mut buffer: ResMut<StrategyBuffer>,
    mut run_events: EventWriter<StrategyRunRequested>,
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
                save_clicked = ui
                    .add_enabled(can_save, egui::Button::new("Save Cache"))
                    .clicked();
                run_clicked = ui.add_enabled(can_run, egui::Button::new("Run")).clicked();

                if let Some(path) = &cache_path_clone {
                    ui.label(format!(
                        "cache: {}",
                        path.file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<cache>")
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

// ── Bevy native 版 Strategy Editor（Sub-step 1.8b 以降） ─────────────
// 旧 egui 版 (strategy_editor_window_system) は 1.8d で削除予定。
// それまで両方並行稼働。

const PANEL_SIZE: Vec2 = Vec2::new(500.0, 400.0);
const PANEL_POSITION: Vec2 = Vec2::new(-300.0, 50.0);
const EDITOR_SIZE: Vec2 = Vec2::new(440.0, 320.0);
const ACCENT: Color = Color::srgba(0.63, 0.44, 1.0, 0.4); // SVG #a070ff (purple)
const EDITOR_BG: Color = Color::srgba(0.02, 0.02, 0.04, 1.0);

/// エディタ本体（TextEdit2d 付き sprite）を識別するマーカー。
/// Sub-step 1.8c で `Query<&mut CosmicEditBuffer, With<StrategyEditorContent>>` で取りに行く。
#[derive(Component)]
pub struct StrategyEditorContent;

/// dispatcher から呼ばれる spawn 関数。
pub fn spawn_strategy_editor_panel(commands: &mut Commands, font_system: &mut CosmicFontSystem) {
    let (root, content_area) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "STRATEGY EDITOR".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
        },
    );
    commands.entity(root).insert(PanelKind::StrategyEditor);

    // bevy_cosmic_edit の TextEdit2d。Sprite + CosmicEditBuffer は自動で required components として付く。
    let editor = commands
        .spawn((
            TextEdit2d,
            Sprite {
                custom_size: Some(EDITOR_SIZE),
                color: EDITOR_BG,
                ..default()
            },
            CosmicEditBuffer::new(font_system, Metrics::new(14.0, 18.0)).with_text(
                font_system,
                "// strategy code\n",
                Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
            ),
            DefaultAttrs(AttrsOwned::new(
                Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
            )),
            CursorColor(Color::WHITE),
            Transform::from_xyz(0.0, 0.0, 0.1),
            StrategyEditorContent,
        ))
        .id();

    commands.entity(content_area).add_child(editor);
    commands.insert_resource(FocusedWidget(Some(editor)));
}
