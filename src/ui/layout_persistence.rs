use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

use crate::ui::components::{
    OpenStrategyRequested, PanelKind, PanelSpawnRequested, PanelSpawnSource, PendingStrategyLoad,
    StrategyBuffer, WindowManager, WindowRoot,
};

pub const SCHEMA_VERSION: u32 = 1;

/// サイドカー JSON の全フィールドを optional で保持する構造体。
///
/// `<strategy>.json` は「layout-only」「scenario-only」「両方入り」の 3 状態を取る
/// （Phase 7.3 Scenario Sidecar Migration 参照）。layout フィールドを全て Option 化する
/// ことで、scenario-only JSON でも `serde_json::from_str` が成功するようにする。
///
/// `windows: None` = layout キー不在 → `apply_layout_system` で no-op
/// `windows: Some(vec![])` = 明示的に全 window を閉じる
/// `scenario: Option<serde_json::Value>` = layout 側は読まないが save 時に保持して書き戻す
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SidecarLayout {
    /// layout サイドカー全体のスキーマバージョン（layout-only / 両方入り）
    #[serde(default)]
    pub schema_version: Option<u32>,
    /// カメラ pan/zoom。None = キー不在 → apply 時にカメラを触らない
    #[serde(default)]
    pub viewport: Option<ViewportState>,
    /// floating window 配置。
    /// None = キー不在 → apply 時に despawn/spawn しない（既存パネルを保持）
    /// Some(vec![]) = 明示的に全 window を閉じる
    #[serde(default)]
    pub windows: Option<Vec<WindowLayout>>,
    /// ロード時に復元する strategy ファイルのパス（文字列）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_path: Option<String>,
    /// サイドバーで選択中だった銘柄シンボル（例: "7203.T"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_symbol: Option<String>,
    /// SCENARIO の passthrough フィールド。
    /// layout 側は内容を読まないが、save 時に既存 JSON から回収して書き戻す（F1 対応）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<serde_json::Value>,
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

/// `OpenStrategyRequested` を受け取った同フレームは panel spawn が終わっていない可能性がある。
/// sidecar JSON ロードを 1 フレーム遅延させるためのキュー。
///
/// `path` に Some が入っていると `auto_load_sidecar_system` が翌フレームで
/// `LayoutLoadRequested` を発火し、自身を None にリセットする。
#[derive(Resource, Default, Debug, Clone)]
pub struct PendingLayoutLoad {
    pub path: Option<std::path::PathBuf>,
}

/// デバウンス自動保存の状態管理。
///
/// パネルドラッグ終了などで `dirty = true` にセットし、
/// `debounced_autosave_system` が 1 秒後に自動保存する。
#[derive(Resource, Default)]
pub struct AutoSaveState {
    /// 未保存変更があるかどうか
    pub dirty: bool,
    /// 最後に dirty になった時刻
    pub last_change: Option<Instant>,
}

/// 起動時の sidecar 自動ロードをワンショットに制限するフラグ。
///
/// `watch_open_strategy_for_sidecar_system` がこのフラグを確認し、
/// `done == true` なら以降の `OpenStrategyRequested` イベントを読み捨てる。
/// これにより `apply_layout_system` が strategy_path を復元した後に
/// 再び `OpenStrategyRequested` が発火しても sidecar ロードが無限ループしない。
#[derive(Resource, Default)]
pub struct SidecarAutoLoadState {
    pub done: bool,
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

/// ECS 状態から `SidecarLayout` を組み立てる。
///
/// `preserve_scenario_from` に `Some(path)` を渡すと、その `path.with_extension("json")`
/// から既存 `scenario` キーを回収して新 layout に含める（F1 対応）。
/// `None` を渡すと `scenario` は `None`（Save As などで scenario を運ばない場合）。
#[allow(clippy::type_complexity)]
fn build_layout(
    panels: &Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: &Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: &Res<StrategyBuffer>,
    preserve_scenario_from: Option<&std::path::Path>,
) -> SidecarLayout {
    let viewport = camera
        .get_single()
        .map(|(cam_tf, proj)| ViewportState {
            pan_x: cam_tf.translation.x,
            pan_y: cam_tf.translation.y,
            zoom: proj.scale,
        })
        .unwrap_or_default();

    let windows: Vec<WindowLayout> = panels
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

    let strategy_path = buffer
        .original_path
        .as_ref()
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    // 既存サイドカーから scenario キーを回収して merge（F1: save 時に scenario が消えるのを防ぐ）
    let scenario = preserve_scenario_from
        .map(|p| p.with_extension("json"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("scenario").cloned());

    SidecarLayout {
        schema_version: Some(SCHEMA_VERSION),
        viewport: Some(viewport),
        windows: Some(windows),
        strategy_path,
        // 将来の Phase で選択銘柄を収集・復元する予定
        selected_symbol: None,
        scenario,
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

#[allow(clippy::type_complexity)]
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

        let layout = build_layout(&panels, &camera, &buffer, buffer.original_path.as_deref());
        match save_layout_to(&path, &layout) {
            Ok(()) => info!("layout saved to {:?}", path),
            Err(e) => error!("layout save failed: {e}"),
        }
    }
}

#[allow(clippy::type_complexity)]
fn handle_save_as_layout_system(
    mut events: EventReader<LayoutSaveAsRequested>,
    panels: Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: Res<StrategyBuffer>,
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

        // Save As は別ファイルへの保存なので scenario を運ばない（None）
        let layout = build_layout(&panels, &camera, &buffer, None);
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

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
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
    mut pending_strategy: ResMut<PendingStrategyLoad>,
) {
    for event in events.read() {
        let layout = match load_layout_from(&event.path) {
            Ok(l) => l,
            Err(e) => {
                error!("layout load failed from {:?}: {e}", event.path);
                continue;
            }
        };

        // schema_version: None（不在）は ERROR を出さず debug ログに留める（F10: scenario-only JSON 対応）
        match layout.schema_version {
            None => {
                debug!(
                    "layout JSON {:?} has no 'schema_version' key — treating as scenario-only sidecar",
                    event.path
                );
            }
            Some(v) if v != SCHEMA_VERSION => {
                warn!(
                    "layout schema version mismatch: file={v}, expected={SCHEMA_VERSION}. skipping layout apply.",
                );
                continue;
            }
            _ => {}
        }

        // viewport: None → カメラを触らない（F10: scenario-only JSON で camera reset を防ぐ）
        if let (Some(vp), Ok((mut cam_tf, mut proj))) =
            (&layout.viewport, camera.get_single_mut())
        {
            cam_tf.translation.x = vp.pan_x;
            cam_tf.translation.y = vp.pan_y;
            proj.scale = vp.zoom;
        }

        // windows: None → despawn/spawn を一切しない（F10: 既存パネルを消さない）
        // windows: Some(list) → 既存ロジック通り（list に無いパネルを despawn）
        if let Some(win_layouts) = &layout.windows {
            let mut new_max_z = wm.max_z;
            for win_layout in win_layouts {
                let found = panels
                    .iter_mut()
                    .find(|(_, kind, _, _, _)| **kind == win_layout.kind);

                match found {
                    None => {
                        // ECS にまだ存在しない → spawn を要求し、翌フレームで位置適用
                        spawn_ev.send(PanelSpawnRequested {
                            kind: win_layout.kind,
                            source: PanelSpawnSource::LayoutLoad,
                        });
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

            // windows リストに含まれないパネルを despawn（Some([]) で全 despawn も含む）
            let to_despawn: Vec<Entity> = panels
                .iter()
                .filter(|(_, kind, _, _, _)| !win_layouts.iter().any(|w| w.kind == **kind))
                .map(|(entity, _, _, _, _)| entity)
                .collect();
            for entity in to_despawn {
                commands.entity(entity).despawn_recursive();
            }
        }

        // ストラテジーファイルの復元
        if let Some(path_str) = &layout.strategy_path {
            let path = std::path::PathBuf::from(path_str);
            if path.exists() {
                pending_strategy.path = Some(path);
            } else {
                warn!("layout load: strategy_path {:?} not found, skipping", path);
            }
        }

        info!("layout applied from {:?}", event.path);
    }
}

fn apply_pending_layout_system(
    mut pending: ResMut<PendingLayoutApply>,
    mut panels: Query<(&PanelKind, &mut Transform, &mut Sprite, &mut Visibility), With<WindowRoot>>,
    mut wm: ResMut<WindowManager>,
) {
    if pending.windows.is_empty() {
        return;
    }
    let mut still_pending = vec![];
    for win_layout in pending.windows.drain(..) {
        let found = panels
            .iter_mut()
            .find(|(kind, ..)| **kind == win_layout.kind);
        match found {
            None => still_pending.push(win_layout),
            Some((_, mut tf, mut sprite, mut vis)) => {
                tf.translation.x = win_layout.position[0];
                tf.translation.y = win_layout.position[1];
                tf.translation.z = win_layout.z;
                sprite.custom_size = Some(Vec2::from_array(win_layout.size));
                *vis = if win_layout.visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if win_layout.z > wm.max_z {
                    wm.max_z = win_layout.z;
                }
            }
        }
    }
    pending.windows = still_pending;
}

#[allow(clippy::type_complexity)]
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
        let layout = build_layout(&panels, &camera, &buffer, buffer.original_path.as_deref());
        match save_layout_to(&path, &layout) {
            Ok(()) => info!("layout auto-saved to {:?}", path),
            Err(e) => error!("layout auto-save failed: {e}"),
        }
    }
}

/// パネルのドラッグ終了を検知して `AutoSaveState` を dirty にする。
///
/// `despawn_recursive()` 実装なので `Changed<Visibility>` は使わず、
/// `Pointer<DragEnd>` だけを監視する。
fn mark_dirty_on_drag_system(
    mut trigger: Trigger<Pointer<DragEnd>>,
    windows: Query<(), With<WindowRoot>>,
    mut auto_save: ResMut<AutoSaveState>,
) {
    // DragEnd が WindowRoot を持つ entity で発生したときのみ dirty にする
    if windows.get(trigger.entity()).is_ok() {
        auto_save.dirty = true;
        auto_save.last_change = Some(Instant::now());
    }
    trigger.propagate(false);
}

/// dirty かつ最終変更から 1 秒以上経過していたら sidecar JSON に自動保存する。
#[allow(clippy::type_complexity)]
fn debounced_autosave_system(
    mut auto_save: ResMut<AutoSaveState>,
    panels: Query<(&PanelKind, &Transform, &Sprite, &Visibility), With<WindowRoot>>,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: Res<StrategyBuffer>,
) {
    if !auto_save.dirty {
        return;
    }
    let elapsed = auto_save
        .last_change
        .map(|t| t.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    if elapsed < 1.0 {
        return;
    }

    // sidecar JSON は strategy ファイルと同名 .json
    let Some(orig) = &buffer.original_path else {
        // strategy 未選択時は自動保存しない
        auto_save.dirty = false;
        auto_save.last_change = None;
        return;
    };
    let path = orig.with_extension("json");
    let layout = build_layout(&panels, &camera, &buffer, buffer.original_path.as_deref());
    match save_layout_to(&path, &layout) {
        Ok(()) => info!("debounced autosave → {:?}", path),
        Err(e) => error!("debounced autosave failed: {e}"),
    }
    auto_save.dirty = false;
    auto_save.last_change = None;
}

/// `OpenStrategyRequested` を監視し、同名の `.json` が存在すれば
/// `PendingLayoutLoad` にパスをセットする（1 フレーム遅延ロードのため）。
///
/// `SidecarAutoLoadState::done` が true の場合はイベントを読み捨てて即リターンする。
/// これにより `apply_layout_system` が strategy_path 復元のために
/// `OpenStrategyRequested` を再発火しても sidecar ロードが無限ループしない。
fn watch_open_strategy_for_sidecar_system(
    mut events: EventReader<OpenStrategyRequested>,
    mut state: ResMut<SidecarAutoLoadState>,
    mut pending: ResMut<PendingLayoutLoad>,
) {
    if state.done {
        // ワンショット済み: イベントを消費して次フレームに残さない
        for _ in events.read() {}
        return;
    }
    for event in events.read() {
        let sidecar = event.path.with_extension("json");
        if sidecar.exists() {
            info!(
                "sidecar JSON found: {:?} — queueing PendingLayoutLoad",
                sidecar
            );
            pending.path = Some(sidecar);
            // ワンショット: 以降の OpenStrategyRequested は sidecar ロードをスキップ
            state.done = true;
        }
    }
}

/// `PendingLayoutLoad` に path が入っていれば `LayoutLoadRequested` を発火し、
/// resource をリセットする（1 フレーム後に実行されることで panel spawn 待ちを回避）。
fn auto_load_sidecar_system(
    mut pending: ResMut<PendingLayoutLoad>,
    mut writer: EventWriter<LayoutLoadRequested>,
) {
    if let Some(path) = pending.path.take() {
        info!("auto_load_sidecar: firing LayoutLoadRequested for {:?}", path);
        writer.send(LayoutLoadRequested { path });
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
        app.init_resource::<PendingLayoutApply>()
            .init_resource::<PendingLayoutLoad>()
            .init_resource::<AutoSaveState>()
            .init_resource::<SidecarAutoLoadState>()
            .add_event::<LayoutSaveRequested>()
            .add_event::<LayoutSaveAsRequested>()
            .add_event::<LayoutLoadDialogRequested>()
            .add_event::<LayoutLoadRequested>()
            // グローバル observer: WindowRoot の DragEnd で dirty フラグを立てる
            .add_observer(mark_dirty_on_drag_system)
            .add_systems(
                Update,
                (
                    handle_save_layout_system,
                    handle_save_as_layout_system,
                    handle_load_dialog_system,
                    // デバウンス自動保存
                    debounced_autosave_system,
                    // sidecar 監視 → pending セット → 翌フレームでロード
                    watch_open_strategy_for_sidecar_system,
                    auto_load_sidecar_system.after(watch_open_strategy_for_sidecar_system),
                    apply_layout_system.after(auto_load_sidecar_system),
                    apply_pending_layout_system,
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
            schema_version: Some(SCHEMA_VERSION),
            viewport: Some(ViewportState {
                pan_x: 10.0,
                pan_y: -20.0,
                zoom: 1.5,
            }),
            windows: Some(vec![
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
            ]),
            strategy_path: None,
            selected_symbol: None,
            scenario: None,
        };
        let json = serde_json::to_string_pretty(&layout).unwrap();
        let restored: SidecarLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout.schema_version, restored.schema_version);
        assert_eq!(layout.windows.as_ref().map(|v| v.len()), restored.windows.as_ref().map(|v| v.len()));
        assert!(restored.scenario.is_none());
    }

    /// scenario-only JSON（`{"scenario": {...}}`）が deserialize で成功し、
    /// windows / viewport が None になること（F10: 全パネル despawn 事故を防ぐ）
    #[test]
    fn test_deserialize_scenario_only_sidecar() {
        let json = r#"{"scenario": {"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}"#;
        let layout: SidecarLayout = serde_json::from_str(json).unwrap();
        assert!(layout.windows.is_none(), "windows must be None for scenario-only sidecar");
        assert!(layout.viewport.is_none(), "viewport must be None for scenario-only sidecar");
        assert!(layout.scenario.is_some(), "scenario field must be preserved");
    }

    /// 既存 layout-only JSON（旧形式）が今まで通り読めること
    #[test]
    fn test_deserialize_layout_only_sidecar() {
        let json = r#"{"schema_version": 1, "viewport": {"pan_x": 0.0, "pan_y": 0.0, "zoom": 1.0}, "windows": [{"kind": "Chart", "visible": true, "position": [0.0, 0.0], "size": [400.0, 300.0], "z": 1.0}]}"#;
        let layout: SidecarLayout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.schema_version, Some(1));
        assert!(layout.viewport.is_some());
        assert!(layout.windows.is_some());
        assert!(layout.scenario.is_none());
    }

    /// 両方入り JSON が正しく分離して読めること
    #[test]
    fn test_deserialize_combined_sidecar() {
        let json = r#"{
            "schema_version": 1,
            "scenario": {"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000},
            "viewport": {"pan_x": 1.0, "pan_y": 2.0, "zoom": 1.5},
            "windows": []
        }"#;
        let layout: SidecarLayout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.schema_version, Some(1));
        assert!(layout.scenario.is_some());
        assert!(layout.viewport.is_some());
        // windows は Some(vec![])（明示的空配列）
        assert!(layout.windows.is_some());
        assert!(layout.windows.as_ref().unwrap().is_empty());
    }

    /// scenario キー付き JSON を serialize すると scenario が保持されること（F1 round-trip）
    #[test]
    fn test_save_layout_preserves_scenario_key() {
        let scenario_val: serde_json::Value = serde_json::from_str(
            r#"{"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}"#
        ).unwrap();
        let layout = SidecarLayout {
            schema_version: Some(1),
            viewport: Some(ViewportState::default()),
            windows: Some(vec![]),
            strategy_path: None,
            selected_symbol: None,
            scenario: Some(scenario_val),
        };
        let json = serde_json::to_string_pretty(&layout).unwrap();
        let restored: SidecarLayout = serde_json::from_str(&json).unwrap();
        assert!(restored.scenario.is_some(), "scenario must survive round-trip");
        let sc = restored.scenario.unwrap();
        assert_eq!(sc["instrument"], "1301.TSE");
    }

    /// Save As は scenario を運ばない（None を渡した場合）
    #[test]
    fn test_save_as_does_not_carry_scenario() {
        // preserve_scenario_from: None の場合、scenario フィールドは None
        let layout = SidecarLayout {
            schema_version: Some(1),
            viewport: Some(ViewportState::default()),
            windows: Some(vec![]),
            strategy_path: None,
            selected_symbol: None,
            scenario: None, // Save As は scenario を運ばない
        };
        let json = serde_json::to_string_pretty(&layout).unwrap();
        // JSON 文字列に "scenario" キーが含まれないこと（skip_serializing_if = None）
        assert!(
            !json.contains("\"scenario\""),
            "Save As should not carry scenario key"
        );
    }

    /// build_layout が既存 sidecar から scenario を回収する場合の結果検証
    /// (tmp ファイルを使った間接テスト)
    #[test]
    fn test_build_layout_recovers_scenario_from_existing_sidecar() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let json_path = dir.join("test_scenario_recovery.json");
        let mut f = std::fs::File::create(&json_path).unwrap();
        writeln!(f, r#"{{"scenario": {{"schema_version": 1, "instrument": "7203.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}}}"#).unwrap();
        drop(f);

        // py_path.with_extension("json") が json_path になるような py_path を作る
        let py_path = json_path.with_extension("py");

        // preserve_scenario_from に py_path を渡すと scenario が回収される
        let scenario = Some(py_path.as_path())
            .map(|p| p.with_extension("json"))
            .filter(|p| p.exists())
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("scenario").cloned());

        assert!(scenario.is_some(), "scenario must be recovered from existing sidecar");
        assert_eq!(scenario.unwrap()["instrument"], "7203.TSE");

        std::fs::remove_file(&json_path).ok();
    }
}
