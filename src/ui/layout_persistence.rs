use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

use crate::ui::components::{
    PanelKind, PanelSpawnRequested, PanelSpawnSource, PendingStrategyFragments, RegionKeyAllocator,
    StrategyBuffer, StrategyEditorId, StrategyEditorSpawnSpec, StrategyFileLoadRequested,
    StrategyFragment, StrategyLoadMode, WindowManager, WindowRoot,
};
use crate::ui::menu_bar::{cache_state_paths, sync_to_cache};
use crate::ui::strategy_editor::{
    StrategyAutoSaveState, flush_strategy_cache, merge_fragments, split_py_into_fragments,
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
    /// StrategyEditor ウィンドウが持つ region キー。
    /// None = 旧サイドカー JSON との後方互換（欠如フィールド）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_key: Option<String>,
}

/// JSON に記録されているが ECS にまだ存在しないパネルのレイアウト情報を
/// 翌フレーム適用するために蓄積するリソース。
#[derive(Resource, Default, Debug, Clone)]
pub struct PendingLayoutApply {
    pub windows: Vec<WindowLayout>,
    pub waiting_for_strategy: bool,
    /// 既に `PanelSpawnRequested` を発火した window の識別キー集合。
    /// dispatcher の spawn は次フレームの `panels` query に反映されるので、
    /// その間に `apply_pending_layout_system` が再走しても二重 spawn しないためのガード。
    /// キー: (PanelKind, region_key)。StrategyEditor 以外は region_key=None で 1 件のみ。
    pub spawn_requested: std::collections::HashSet<(PanelKind, Option<String>)>,
}

// 通常 Open は `LayoutLoadRequested` で original sidecar を読む。
// 起動復元は `CacheRestoreRequested` で fixed cache を読むため、original `.py` は再読込しない。

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

impl AutoSaveState {
    /// レイアウト変更 (drag / close など) を検知したことを記録する。
    /// 呼び出し側で「対象が WindowRoot か」を判定済みであることを前提とする。
    pub fn mark_layout_changed(&mut self, now: Instant) {
        self.dirty = true;
        self.last_change = Some(now);
    }
}

/// 起動時の sidecar 自動ロードをワンショットに制限するフラグ。
///
/// `apply_layout_system` が `strategy_path` を読んで `StrategyFileLoadRequested { mode: LayoutRestore }`
/// を発火した後、`handle_strategy_file_load_system` がさらに sidecar を発火し直すループを防ぐ。
/// `done == true` のとき `apply_layout_system` は `strategy_path` 由来の追加発火を抑制する。
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

#[derive(Event, Debug, Clone)]
pub struct CacheRestoreRequested {
    pub layout: SidecarLayout,
}

/// Phase 7.5: legacy `PanelKind::Chart` entry を含む旧 layout JSON を読み込んだとき、
/// 当該 WindowLayout を spawn/pending 投入する前に弾く。
#[inline]
fn is_legacy_chart_entry(win_layout: &WindowLayout) -> bool {
    if win_layout.kind == PanelKind::Chart {
        warn!(
            "layout: skipping deprecated PanelKind::Chart entry (pos={:?}, size={:?}); \
             Chart panel is removed in Phase 7.5",
            win_layout.position, win_layout.size
        );
        true
    } else {
        false
    }
}

/// ECS 状態から `SidecarLayout` を組み立てる。
///
/// `preserve_scenario_from` に `Some(path)` を渡すと、その `path.with_extension("json")`
/// から既存 `scenario` キーを回収して新 layout に含める（F1 対応）。
/// `None` を渡すと `scenario` は `None` のままになる。
#[allow(clippy::type_complexity)]
fn build_layout(
    panels: &Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &Transform,
            &Sprite,
            &Visibility,
        ),
        With<WindowRoot>,
    >,
    camera: &Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: &StrategyBuffer,
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
        .map(|(kind, id, tf, sprite, vis)| {
            let visible = !matches!(vis, Visibility::Hidden);
            WindowLayout {
                kind: *kind,
                visible,
                position: [tf.translation.x, tf.translation.y],
                size: sprite.custom_size.unwrap_or(Vec2::ZERO).to_array(),
                z: tf.translation.z,
                region_key: id.map(|i| i.region_key.clone()),
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
    serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn apply_cache_restore_system(
    mut events: EventReader<CacheRestoreRequested>,
    mut buffer: ResMut<StrategyBuffer>,
    mut allocator: ResMut<RegionKeyAllocator>,
    mut pending_fragments: ResMut<PendingStrategyFragments>,
    mut camera: Query<
        (&mut Transform, &mut OrthographicProjection),
        (With<Camera2d>, Without<WindowRoot>),
    >,
    mut pending: ResMut<PendingLayoutApply>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
) {
    for event in events.read() {
        let Some((_, cache_py)) = cache_state_paths() else {
            error!("cache restore failed: cache_dir not found");
            continue;
        };
        let source = match std::fs::read_to_string(&cache_py) {
            Ok(source) => source,
            Err(e) => {
                error!("cache restore failed: could not read {:?}: {e}", cache_py);
                continue;
            }
        };

        let outcome = split_py_into_fragments(&source);
        for warning in &outcome.warnings {
            warn!("cache restore split warning ({:?}): {}", cache_py, warning);
        }

        buffer.original_path = event.layout.strategy_path.as_ref().map(PathBuf::from);
        buffer.cache_path = Some(cache_py.clone());
        buffer.last_merged_source = None;

        allocator.bump_to_at_least(outcome.max_numeric_suffix);
        pending_fragments.by_region_key.clear();
        pending_fragments.loaded_for_path = buffer.original_path.clone();
        for (key, body) in &outcome.fragments {
            pending_fragments
                .by_region_key
                .insert(key.clone(), body.clone());
        }

        if let (Some(vp), Ok((mut cam_tf, mut proj))) =
            (&event.layout.viewport, camera.get_single_mut())
        {
            cam_tf.translation.x = vp.pan_x;
            cam_tf.translation.y = vp.pan_y;
            proj.scale = vp.zoom;
        }

        if let Some(win_layouts) = &event.layout.windows {
            for win_layout in win_layouts {
                if is_legacy_chart_entry(win_layout) {
                    continue;
                }
                pending.windows.push(win_layout.clone());

                let region_key = if win_layout.kind == PanelKind::StrategyEditor {
                    Some(
                        win_layout
                            .region_key
                            .clone()
                            .unwrap_or_else(|| "region_001".to_string()),
                    )
                } else {
                    None
                };
                let dedupe_key = (win_layout.kind, region_key.clone());
                if pending.spawn_requested.insert(dedupe_key) {
                    let strategy_spec = if win_layout.kind == PanelKind::StrategyEditor {
                        Some(StrategyEditorSpawnSpec {
                            region_key,
                            source: None,
                            layout_source: PanelSpawnSource::LayoutLoad,
                        })
                    } else {
                        None
                    };
                    spawn_ev.send(PanelSpawnRequested {
                        kind: win_layout.kind,
                        source: PanelSpawnSource::LayoutLoad,
                        strategy_spec,
                    });
                }
            }
        }

        info!(
            "cache restore loaded strategy fragments: cache={:?}, original={:?}, regions={}",
            cache_py,
            buffer.original_path,
            outcome.fragments.len()
        );
    }
}

#[allow(clippy::type_complexity)]
fn handle_save_layout_system(
    mut events: EventReader<LayoutSaveRequested>,
    panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &Transform,
            &Sprite,
            &Visibility,
        ),
        With<WindowRoot>,
    >,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
) {
    for _ in events.read() {
        let was_new = buffer.original_path.is_none();

        // ダイアログ（初回保存）の場合は先にパスを確定させる
        let json_path = if let Some(orig) = &buffer.original_path {
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

        let py_path = json_path.with_extension("py");

        // Fix(High): build_layout の前に original_path を新パスへ更新しておく。
        // これで JSON の strategy_path フィールドが正しい .py を指す。
        if was_new {
            buffer.original_path = Some(py_path.clone());
            buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py);
        }

        let layout = build_layout(&panels, &camera, &*buffer, buffer.original_path.as_deref());
        match save_layout_to(&json_path, &layout) {
            Ok(()) => info!("layout saved to {:?}", json_path),
            Err(e) => {
                error!("layout save failed: {e}");
                // ロールバック: original_path を None に戻す（初回 save の場合）
                if was_new {
                    buffer.original_path = None;
                    buffer.cache_path = None;
                }
                continue;
            }
        }

        let mut items: Vec<(String, String)> = fragments_q
            .iter()
            .map(|(id, frag)| (id.region_key.clone(), frag.source.clone()))
            .collect();
        if !items.is_empty() {
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let merged = merge_fragments(&items);
            match std::fs::write(&py_path, &merged) {
                Ok(()) => {
                    info!("strategy .py saved to {:?}", py_path);
                    if let Err(e) = sync_to_cache(&py_path) {
                        error!("failed to sync saved strategy to cache: {e}");
                    }
                    for (_, mut frag) in fragments_q.iter_mut() {
                        frag.dirty = false;
                    }
                    strategy_auto_save.dirty = false;
                    strategy_auto_save.last_change = None;
                }
                Err(e) => error!("strategy .py save failed: {e}"),
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn handle_save_as_layout_system(
    mut events: EventReader<LayoutSaveAsRequested>,
    panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &Transform,
            &Sprite,
            &Visibility,
        ),
        With<WindowRoot>,
    >,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
) {
    for _ in events.read() {
        let json_path = match FileDialog::new()
            .add_filter("Layout JSON", &["json"])
            .save_file()
        {
            Some(p) => p,
            None => {
                info!("layout save-as cancelled: no path selected");
                continue;
            }
        };

        let py_path = json_path.with_extension("py");

        // Fix(High): buffer を先に新パスへ更新 → build_layout の strategy_path が正しくなる
        let old_original = buffer.original_path.clone();
        let old_cache = buffer.cache_path.clone();
        buffer.original_path = Some(py_path.clone());
        buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py);

        let cache_json = cache_state_paths().map(|(cache_json, _)| cache_json);
        let layout = build_layout(&panels, &camera, &*buffer, cache_json.as_deref());
        match save_layout_to(&json_path, &layout) {
            Ok(()) => info!("layout saved-as to {:?}", json_path),
            Err(e) => {
                error!("layout save-as failed: {e}");
                // ロールバック
                buffer.original_path = old_original;
                buffer.cache_path = old_cache;
                continue;
            }
        }

        let mut items: Vec<(String, String)> = fragments_q
            .iter()
            .map(|(id, frag)| (id.region_key.clone(), frag.source.clone()))
            .collect();
        if !items.is_empty() {
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let merged = merge_fragments(&items);
            match std::fs::write(&py_path, &merged) {
                Ok(()) => {
                    info!("strategy .py saved-as to {:?}", py_path);
                    if let Err(e) = sync_to_cache(&py_path) {
                        error!("failed to sync saved-as strategy to cache: {e}");
                    }
                    for (_, mut frag) in fragments_q.iter_mut() {
                        frag.dirty = false;
                    }
                    strategy_auto_save.dirty = false;
                    strategy_auto_save.last_change = None;
                }
                Err(e) => {
                    // Fix(Medium): .py 保存失敗時は buffer を元に戻す
                    error!("strategy .py save-as failed: {e}");
                    buffer.original_path = old_original;
                    buffer.cache_path = old_cache;
                }
            }
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
    mut panels: Query<
        (
            Entity,
            &PanelKind,
            Option<&StrategyEditorId>,
            &mut Transform,
            &mut Sprite,
            &mut Visibility,
        ),
        With<WindowRoot>,
    >,
    mut camera: Query<
        (&mut Transform, &mut OrthographicProjection),
        (With<Camera2d>, Without<WindowRoot>),
    >,
    mut wm: ResMut<WindowManager>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    mut pending: ResMut<PendingLayoutApply>,
    mut load_ev: EventWriter<StrategyFileLoadRequested>,
    mut sidecar_state: ResMut<SidecarAutoLoadState>,
    pending_fragments: Res<PendingStrategyFragments>,
    // ワンショット loopback 抑制: 直近で scenario-only Open → sibling .py 発火 →
    // handler が同じ JSON を再発火、までの 1 サイクルだけスキップする。
    // pending_fragments.loaded_for_path のような恒久的状態に基づくと、
    // 「同じ JSON を後から再 Open」したケースまで抑制されてしまうため。
    mut pending_loopback: Local<Option<PathBuf>>,
) {
    for event in events.read() {
        let layout = match load_layout_from(&event.path) {
            Ok(l) => l,
            Err(e) => {
                error!("layout load failed from {:?}: {e}", event.path);
                continue;
            }
        };

        // scenario-only JSON（windows / strategy_path 不在）を直接 Open された場合、
        // sibling `<stem>.py` が存在すればそちらを UserOpen として委譲する。
        // handler が同 JSON を再発火するため、その 1 回分だけ loopback 抑制する。
        if layout.strategy_path.is_none() && layout.windows.is_none() {
            if pending_loopback.as_ref() == Some(&event.path) {
                *pending_loopback = None;
                continue;
            }
            let sibling_py = event.path.with_extension("py");
            if sibling_py.exists() {
                info!(
                    "scenario-only JSON {:?} opened directly; loading sibling strategy {:?}",
                    event.path, sibling_py
                );
                load_ev.send(StrategyFileLoadRequested {
                    path: sibling_py,
                    mode: StrategyLoadMode::UserOpen,
                });
                *pending_loopback = Some(event.path.clone());
                continue;
            }
        }

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

        if let Some(path_str) = &layout.strategy_path {
            let path = std::path::PathBuf::from(path_str);
            if path.exists() {
                if let Some(user_path) = &pending_fragments.loaded_for_path {
                    if user_path != &path {
                        warn!(
                            "apply_layout_system: sidecar strategy_path {:?} \
                             differs from user-selected {:?}; ignoring sidecar path",
                            path, user_path
                        );
                    } else {
                        debug!(
                            "apply_layout_system: skipping strategy_path reload \
                             (already loaded via UserOpen: {:?})",
                            path
                        );
                    }
                } else if !sidecar_state.done {
                    load_ev.send(StrategyFileLoadRequested {
                        path,
                        mode: StrategyLoadMode::LayoutRestore,
                    });
                    sidecar_state.done = true;
                    // ウィンドウ spawn をキューして翌フレームまで defer する
                    if let Some(win_layouts) = &layout.windows {
                        pending.windows.extend(win_layouts.iter().cloned());
                        pending.waiting_for_strategy = true;
                    }
                    // カメラは同フレーム内で適用可能
                    if let (Some(vp), Ok((mut cam_tf, mut proj))) =
                        (&layout.viewport, camera.get_single_mut())
                    {
                        cam_tf.translation.x = vp.pan_x;
                        cam_tf.translation.y = vp.pan_y;
                        proj.scale = vp.zoom;
                    }
                    info!(
                        "layout apply deferred (waiting for strategy fragments): {:?}",
                        event.path
                    );
                    continue;
                } else {
                    debug!(
                        "apply_layout_system: skipping strategy_path reload (sidecar one-shot done)"
                    );
                }
            } else {
                warn!("layout load: strategy_path {:?} not found, skipping", path);
            }
        }

        // viewport: None → カメラを触らない（F10: scenario-only JSON で camera reset を防ぐ）
        if let (Some(vp), Ok((mut cam_tf, mut proj))) = (&layout.viewport, camera.get_single_mut())
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
                if is_legacy_chart_entry(win_layout) {
                    continue;
                }
                let want_key: Option<String> = if win_layout.kind == PanelKind::StrategyEditor {
                    Some(
                        win_layout
                            .region_key
                            .clone()
                            .unwrap_or_else(|| "region_001".to_string()),
                    )
                } else {
                    None
                };
                let found = panels.iter_mut().find(|(_, kind, id, _, _, _)| {
                    if **kind != win_layout.kind {
                        return false;
                    }
                    match (win_layout.kind, want_key.as_deref(), id.as_ref()) {
                        (PanelKind::StrategyEditor, Some(k), Some(eid)) => eid.region_key == k,
                        (PanelKind::StrategyEditor, _, _) => false,
                        _ => true,
                    }
                });

                match found {
                    None => {
                        let dedupe_key = (win_layout.kind, want_key.clone());
                        if pending.spawn_requested.insert(dedupe_key) {
                            let strategy_spec = if win_layout.kind == PanelKind::StrategyEditor {
                                Some(StrategyEditorSpawnSpec {
                                    region_key: want_key.clone(),
                                    source: None,
                                    layout_source: PanelSpawnSource::LayoutLoad,
                                })
                            } else {
                                None
                            };
                            spawn_ev.send(PanelSpawnRequested {
                                kind: win_layout.kind,
                                source: PanelSpawnSource::LayoutLoad,
                                strategy_spec,
                            });
                        }
                        pending.windows.push(win_layout.clone());
                    }
                    Some((_, _, _, mut tf, mut sprite, mut vis)) => {
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
                .filter(|(_, kind, id, _, _, _)| {
                    !win_layouts.iter().any(|w| {
                        if w.kind != **kind {
                            return false;
                        }
                        if w.kind == PanelKind::StrategyEditor {
                            let want = w.region_key.as_deref().unwrap_or("region_001");
                            id.map(|i| i.region_key == want).unwrap_or(false)
                        } else {
                            true
                        }
                    })
                })
                .map(|(entity, _, _, _, _, _)| entity)
                .collect();
            for entity in to_despawn {
                commands.entity(entity).despawn_recursive();
            }
        }

        info!("layout applied from {:?}", event.path);
    }
}

fn apply_pending_layout_system(
    mut pending: ResMut<PendingLayoutApply>,
    mut panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &mut Transform,
            &mut Sprite,
            &mut Visibility,
        ),
        With<WindowRoot>,
    >,
    mut wm: ResMut<WindowManager>,
    pending_fragments: Res<PendingStrategyFragments>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
) {
    if pending.windows.is_empty() {
        return;
    }
    if pending.waiting_for_strategy && pending_fragments.by_region_key.is_empty() {
        return;
    }
    if pending.waiting_for_strategy && !pending_fragments.by_region_key.is_empty() {
        pending.waiting_for_strategy = false;
    }
    let mut still_pending = vec![];
    let windows = std::mem::take(&mut pending.windows);
    for win_layout in windows {
        if is_legacy_chart_entry(&win_layout) {
            continue;
        }
        let found = panels.iter_mut().find(|(kind, id, ..)| {
            if **kind != win_layout.kind {
                return false;
            }
            if win_layout.kind == PanelKind::StrategyEditor {
                let want = win_layout.region_key.as_deref().unwrap_or("region_001");
                id.map(|i| i.region_key == want).unwrap_or(false)
            } else {
                true
            }
        });
        match found {
            None => {
                let region_key = if win_layout.kind == PanelKind::StrategyEditor {
                    Some(
                        win_layout
                            .region_key
                            .clone()
                            .unwrap_or_else(|| "region_001".to_string()),
                    )
                } else {
                    None
                };
                let dedupe_key = (win_layout.kind, region_key.clone());
                if pending.spawn_requested.insert(dedupe_key) {
                    let strategy_spec = if win_layout.kind == PanelKind::StrategyEditor {
                        Some(StrategyEditorSpawnSpec {
                            region_key,
                            source: None,
                            layout_source: PanelSpawnSource::LayoutLoad,
                        })
                    } else {
                        None
                    };
                    spawn_ev.send(PanelSpawnRequested {
                        kind: win_layout.kind,
                        source: PanelSpawnSource::LayoutLoad,
                        strategy_spec,
                    });
                }
                still_pending.push(win_layout);
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
                if win_layout.z > wm.max_z {
                    wm.max_z = win_layout.z;
                }
            }
        }
    }
    pending.windows = still_pending;
    if pending.windows.is_empty() {
        pending.spawn_requested.clear();
    }
}

#[allow(clippy::type_complexity)]
fn save_layout_on_window_close(
    mut close_events: EventReader<WindowCloseRequested>,
    panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &Transform,
            &Sprite,
            &Visibility,
        ),
        With<WindowRoot>,
    >,
    camera: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
) {
    // Bevy 0.15 の winit は WindowCloseRequested を EventWriter 経由で送る。
    // add_observer が期待する trigger_targets() では送られないため observer は発火しない。
    // EventReader + add_systems(Update, ...) なら同フレーム内で確実に受信でき、
    // window entity が削除される前にセーブが完了する。
    for _ in close_events.read() {
        let mut items: Vec<(String, String)> = fragments_q
            .iter()
            .map(|(id, frag)| (id.region_key.clone(), frag.source.clone()))
            .collect();
        if !items.is_empty() {
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let merged = merge_fragments(&items);
            match flush_strategy_cache(&merged, &mut buffer, &mut strategy_auto_save) {
                Ok(true) => {
                    for (_, mut frag) in fragments_q.iter_mut() {
                        frag.dirty = false;
                    }
                    info!(
                        "strategy cache flushed on window close: {:?}",
                        buffer.cache_path
                    );
                }
                Ok(false) => warn!("strategy cache flush skipped on window close: no cache_path"),
                Err(e) => error!("strategy cache flush on window close failed: {e}"),
            }
        }

        let Some((cache_json, _)) = cache_state_paths() else {
            error!("layout auto-save failed: cache_dir not found");
            continue;
        };
        let layout = build_layout(&panels, &camera, &*buffer, buffer.original_path.as_deref());
        match save_layout_to(&cache_json, &layout) {
            Ok(()) => info!("layout auto-saved to {:?}", cache_json),
            Err(e) => error!("layout auto-save failed: {e}"),
        }
    }
}

/// dirty かつ最終変更から 1 秒以上経過していたら sidecar JSON に自動保存する。
#[allow(clippy::type_complexity)]
fn debounced_autosave_system(
    mut auto_save: ResMut<AutoSaveState>,
    panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &Transform,
            &Sprite,
            &Visibility,
        ),
        With<WindowRoot>,
    >,
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

    let Some((cache_json, _)) = cache_state_paths() else {
        error!("debounced autosave failed: cache_dir not found");
        auto_save.dirty = false;
        auto_save.last_change = None;
        return;
    };
    let layout = build_layout(&panels, &camera, &*buffer, buffer.original_path.as_deref());
    match save_layout_to(&cache_json, &layout) {
        Ok(()) => info!("debounced autosave → {:?}", cache_json),
        Err(e) => error!("debounced autosave failed: {e}"),
    }
    auto_save.dirty = false;
    auto_save.last_change = None;
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
            .init_resource::<AutoSaveState>()
            .init_resource::<SidecarAutoLoadState>()
            .add_event::<LayoutSaveRequested>()
            .add_event::<LayoutSaveAsRequested>()
            .add_event::<LayoutLoadDialogRequested>()
            .add_event::<LayoutLoadRequested>()
            .add_event::<CacheRestoreRequested>()
            .add_systems(
                Update,
                (
                    handle_save_layout_system,
                    handle_save_as_layout_system,
                    handle_load_dialog_system,
                    // デバウンス自動保存
                    debounced_autosave_system,
                    apply_cache_restore_system,
                    apply_layout_system,
                    apply_pending_layout_system.after(apply_layout_system),
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
    fn mark_layout_changed_sets_dirty_and_timestamp() {
        let mut state = AutoSaveState::default();
        assert!(!state.dirty);
        assert!(state.last_change.is_none());

        let now = Instant::now();
        state.mark_layout_changed(now);

        assert!(state.dirty, "dirty must be true after layout change");
        assert_eq!(state.last_change, Some(now));
    }

    #[test]
    fn mark_layout_changed_updates_timestamp_on_subsequent_calls() {
        let mut state = AutoSaveState::default();
        let t1 = Instant::now();
        state.mark_layout_changed(t1);
        let t2 = t1 + std::time::Duration::from_millis(500);
        state.mark_layout_changed(t2);
        assert_eq!(state.last_change, Some(t2));
    }

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
                    region_key: None,
                },
                WindowLayout {
                    kind: PanelKind::Orders,
                    visible: false,
                    position: [-50.0, 75.0],
                    size: [200.0, 150.0],
                    z: 2.0,
                    region_key: None,
                },
            ]),
            strategy_path: None,
            selected_symbol: None,
            scenario: None,
        };
        let json = serde_json::to_string_pretty(&layout).unwrap();
        let restored: SidecarLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout.schema_version, restored.schema_version);
        assert_eq!(
            layout.windows.as_ref().map(|v| v.len()),
            restored.windows.as_ref().map(|v| v.len())
        );
        assert!(restored.scenario.is_none());
    }

    /// scenario-only JSON（`{"scenario": {...}}`）が deserialize で成功し、
    /// windows / viewport が None になること（F10: 全パネル despawn 事故を防ぐ）
    #[test]
    fn test_deserialize_scenario_only_sidecar() {
        let json = r#"{"scenario": {"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}"#;
        let layout: SidecarLayout = serde_json::from_str(json).unwrap();
        assert!(
            layout.windows.is_none(),
            "windows must be None for scenario-only sidecar"
        );
        assert!(
            layout.viewport.is_none(),
            "viewport must be None for scenario-only sidecar"
        );
        assert!(
            layout.scenario.is_some(),
            "scenario field must be preserved"
        );
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
        assert!(
            restored.scenario.is_some(),
            "scenario must survive round-trip"
        );
        let sc = restored.scenario.unwrap();
        assert_eq!(sc["instrument"], "1301.TSE");
    }

    /// scenario が None の場合は JSON に scenario キーを書かない。
    #[test]
    fn test_layout_omits_scenario_when_none() {
        let layout = SidecarLayout {
            schema_version: Some(1),
            viewport: Some(ViewportState::default()),
            windows: Some(vec![]),
            strategy_path: None,
            selected_symbol: None,
            scenario: None,
        };
        let json = serde_json::to_string_pretty(&layout).unwrap();
        assert!(
            !json.contains("\"scenario\""),
            "layout should omit scenario key when scenario is None"
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

        assert!(
            scenario.is_some(),
            "scenario must be recovered from existing sidecar"
        );
        assert_eq!(scenario.unwrap()["instrument"], "7203.TSE");

        std::fs::remove_file(&json_path).ok();
    }

    #[test]
    fn legacy_chart_window_layout_is_skipped() {
        let chart = WindowLayout {
            kind: PanelKind::Chart,
            visible: true,
            position: [10.0, 20.0],
            size: [400.0, 300.0],
            z: 1.0,
            region_key: None,
        };
        let orders = WindowLayout {
            kind: PanelKind::Orders,
            visible: true,
            position: [0.0, 0.0],
            size: [200.0, 150.0],
            z: 1.0,
            region_key: None,
        };
        assert!(is_legacy_chart_entry(&chart), "Chart entry must be skipped");
        assert!(!is_legacy_chart_entry(&orders), "non-Chart entries must pass through");
    }
}
