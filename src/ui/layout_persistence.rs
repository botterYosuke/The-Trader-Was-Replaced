use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::ui::components::{PanelKind, PanelSpawnRequested, StrategyBuffer, WindowManager, WindowRoot};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SidecarLayout {
    pub schema_version: u32,
    pub viewport: ViewportState,
    pub windows: Vec<WindowLayout>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewportState {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WindowLayout {
    pub kind: PanelKind,
    pub visible: bool,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub z: f32,
}

/// JSON に記録されているが ECS にまだ存在しないパネルのレイアウト情報を
/// 翌フレーム適用するために蓄積するリソース。
#[derive(Resource, Default, Debug, Clone)]
pub struct PendingLayoutApply {
    pub windows: Vec<WindowLayout>,
}

#[derive(Event, Debug, Clone)]
pub struct LayoutSaveRequested;

#[derive(Event, Debug, Clone)]
pub struct LayoutSaveAsRequested;

#[derive(Event, Debug, Clone)]
pub struct LayoutLoadDialogRequested;

#[derive(Event, Debug, Clone)]
pub struct LayoutLoadRequested {
    pub path: PathBuf,
}

fn build_layout(
    panels: &Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: &Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
) -> SidecarLayout {
    let viewport = camera
        .get_single()
        .map(|(cam_tf, proj)| ViewportState {
            pan_x: cam_tf.translation.x,
            pan_y: cam_tf.translation.y,
            zoom: proj.scale,
        })
        .unwrap_or_default();

    let windows = panels
        .iter()
        .map(|(kind, tf, sprite, vis)| {
            let visible = !matches!(vis, Visibility::Hidden);
            WindowLayout {
                kind: *kind,
                visible,
                position: [tf.translation.x, tf.translation.y],
                size: sprite.custom_size.unwrap_or(Vec2::ZERO).to_array(),
                z: tf.translation.z,
            }
        })
        .collect();

    SidecarLayout {
        schema_version: SCHEMA_VERSION,
        viewport,
        windows,
    }
}

fn save_layout_to(path: &PathBuf, layout: &SidecarLayout) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(layout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

fn load_layout_from(path: &PathBuf) -> std::io::Result<SidecarLayout> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn handle_save_layout_system(
    mut events: EventReader<LayoutSaveRequested>,
    panels: Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: Res<StrategyBuffer>,
) {
    for _ in events.read() {
        let path = if let Some(orig) = &buffer.original_path {
            orig.with_extension("json")
        } else {
            match FileDialog::new()
                .add_filter("Layout JSON", &["json"])
                .save_file()
            {
                Some(p) => p,
                None => {
                    info!("layout save cancelled: no path selected");
                    continue;
                }
            }
        };

        let layout = build_layout(&panels, &camera);
        match save_layout_to(&path, &layout) {
            Ok(()) => info!("layout saved to {:?}", path),
            Err(e) => error!("layout save failed: {e}"),
        }
    }
}

fn handle_save_as_layout_system(
    mut events: EventReader<LayoutSaveAsRequested>,
    panels: Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
) {
    for _ in events.read() {
        let path = match FileDialog::new()
            .add_filter("Layout JSON", &["json"])
            .save_file()
        {
            Some(p) => p,
            None => {
                info!("layout save-as cancelled: no path selected");
                continue;
            }
        };

        let layout = build_layout(&panels, &camera);
        match save_layout_to(&path, &layout) {
            Ok(()) => info!("layout saved-as to {:?}", path),
            Err(e) => error!("layout save-as failed: {e}"),
        }
    }
}

fn handle_load_dialog_system(
    mut events: EventReader<LayoutLoadDialogRequested>,
    mut writer: EventWriter<LayoutLoadRequested>,
) {
    for _ in events.read() {
        if let Some(path) = FileDialog::new()
            .add_filter("Layout JSON", &["json"])
            .pick_file()
        {
            writer.send(LayoutLoadRequested { path });
        } else {
            info!("layout load cancelled: no file selected");
        }
    }
}

fn apply_layout_system(
    mut commands: Commands,
    mut events: EventReader<LayoutLoadRequested>,
    mut panels: Query<(Entity, &PanelKind, &mut Transform, &mut Sprite, &mut Visibility), With<WindowRoot>>,
    mut camera: Query<
        (&mut Transform, &mut OrthographicProjection),
        (With<Camera2d>, Without<WindowRoot>),
    >,
    mut wm: ResMut<WindowManager>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    mut pending: ResMut<PendingLayoutApply>,
) {
    for event in events.read() {
        let layout = match load_layout_from(&event.path) {
            Ok(l) => l,
            Err(e) => {
                error!("layout load failed from {:?}: {e}", event.path);
                continue;
            }
        };

        if layout.schema_version != SCHEMA_VERSION {
            warn!(
                "layout schema version mismatch: file={}, expected={}. skipping.",
                layout.schema_version, SCHEMA_VERSION
            );
            continue;
        }

        if let Ok((mut cam_tf, mut proj)) = camera.get_single_mut() {
            cam_tf.translation.x = layout.viewport.pan_x;
            cam_tf.translation.y = layout.viewport.pan_y;
            proj.scale = layout.viewport.zoom;
        }

        let mut new_max_z = wm.max_z;
        for win_layout in &layout.windows {
            let found = panels
                .iter_mut()
                .find(|(_, kind, _, _, _)| **kind == win_layout.kind);

            match found {
                None => {
                    // ECS にまだ存在しない → spawn を要求し、翌フレームで位置適用
                    spawn_ev.send(PanelSpawnRequested { kind: win_layout.kind });
                    pending.windows.push(win_layout.clone());
                }
                Some((_, _, mut tf, mut sprite, mut vis)) => {
                    tf.translation.x = win_layout.position[0];
                    tf.translation.y = win_layout.position[1];
                    tf.translation.z = win_layout.z;
                    sprite.custom_size = Some(Vec2::from_array(win_layout.size));
                    *vis = if win_layout.visible {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    if win_layout.z > new_max_z {
                        new_max_z = win_layout.z;
                    }
                }
            }
        }
        wm.max_z = new_max_z;

        let to_despawn: Vec<Entity> = panels
            .iter()
            .filter(|(_, kind, _, _, _)| !layout.windows.iter().any(|w| w.kind == **kind))
            .map(|(entity, _, _, _, _)| entity)
            .collect();
        for entity in to_despawn {
            commands.entity(entity).despawn_recursive();
        }

        info!("layout applied from {:?}", event.path);
    }
}

fn save_layout_on_window_close(
    mut close_events: EventReader<WindowCloseRequested>,
    panels: Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: Res<StrategyBuffer>,
) {
    // Bevy 0.15 の winit は WindowCloseRequested を EventWriter 経由で送る。
    // add_observer が期待する trigger_targets() では送られないため observer は発火しない。
    // EventReader + add_systems(Update, ...) なら同フレーム内で確実に受信でき、
    // window entity が削除される前にセーブが完了する。
    for _ in close_events.read() {
        let Some(orig) = &buffer.original_path else {
            info!("layout auto-save skipped: no original_path");
            continue;
        };
        let path = orig.with_extension("json");
        let layout = build_layout(&panels, &camera);
        match save_layout_to(&path, &layout) {
            Ok(()) => info!("layout auto-saved to {:?}", path),
            Err(e) => error!("layout auto-save failed: {e}"),
        }
    }
}

fn layout_shortcut_system(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cooldown: Local<f32>,
    mut save_w: EventWriter<LayoutSaveRequested>,
    mut save_as_w: EventWriter<LayoutSaveAsRequested>,
    mut load_w: EventWriter<LayoutLoadDialogRequested>,
) {
    // Alt+S/A/O は cosmic-edit が文字入力として処理し panic する。
    // Ctrl combo は cosmic-edit がテキスト入力として扱わないため安全。
    // Save: Ctrl+S / Save As: Ctrl+Shift+S / Load: Ctrl+O
    //
    // Windows の OS キーリピートが winit 経由で just_pressed を複数フレームで
    // true にするため、500ms クールダウンで多重発火を防ぐ。
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    if *cooldown > 0.0 {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::KeyS) {
        if shift {
            save_as_w.send(LayoutSaveAsRequested);
        } else {
            save_w.send(LayoutSaveRequested);
        }
        *cooldown = 0.5;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        load_w.send(LayoutLoadDialogRequested);
        *cooldown = 0.5;
    }
}

pub struct LayoutPersistencePlugin;

impl Plugin for LayoutPersistencePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<LayoutSaveRequested>()
            .add_event::<LayoutSaveAsRequested>()
            .add_event::<LayoutLoadDialogRequested>()
            .add_event::<LayoutLoadRequested>()
            .add_systems(
                Update,
                (
                    handle_save_layout_system,
                    handle_save_as_layout_system,
                    handle_load_dialog_system,
                    apply_layout_system,
                    layout_shortcut_system,
                ),
            )
            .add_systems(Update, save_layout_on_window_close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::PanelKind;

    #[test]
    fn sidecar_layout_round_trip() {
        let layout = SidecarLayout {
            schema_version: SCHEMA_VERSION,
            viewport: ViewportState {
                pan_x: 10.0,
                pan_y: -20.0,
                zoom: 1.5,
            },
            windows: vec![
                WindowLayout {
                    kind: PanelKind::Chart,
                    visible: true,
                    position: [100.0, 200.0],
                    size: [400.0, 300.0],
                    z: 1.0,
                },
                WindowLayout {
                    kind: PanelKind::Orders,
                    visible: false,
                    position: [-50.0, 75.0],
                    size: [200.0, 150.0],
                    z: 2.0,
                },
            ],
        };
        let json = serde_json::to_string_pretty(&layout).unwrap();
        let restored: SidecarLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout, restored);
    }
}
