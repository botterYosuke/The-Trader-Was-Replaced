use bevy::prelude::*;
use bevy::tasks::futures_lite::future;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::window::WindowCloseRequested;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

/// 0.16: camera projection は `Projection` enum。viewport zoom（ortho scale）を読み書きする helper。
fn viewport_zoom(projection: &Projection) -> f32 {
    match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    }
}
fn set_viewport_zoom(projection: &mut Projection, zoom: f32) {
    if let Projection::Orthographic(ortho) = projection {
        ortho.scale = zoom;
    }
}

use crate::ui::components::{
    LayoutExcluded, PanelKind, PanelRestoreDriver, PanelSpawnRequested, PanelSpawnSource,
    PendingStrategyFragments,
    RegionKeyAllocator, ScenarioReadTarget, StrategyBuffer, StrategyEditorId,
    StrategyEditorSpawnSpec, StrategyFileLoadRequested, StrategyFragment, StrategyLoadMode,
    WindowManager, WindowRoot,
};
use crate::ui::menu_bar::{cache_state_paths, sync_to_cache};
use crate::ui::screen_window::{px_of, ScreenWindowRoot};
use crate::ui::strategy_editor::{
    StrategyAutoSaveState, StrategyEditorModeHidden, flush_strategy_cache, merge_fragments,
    split_py_into_fragments,
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

#[derive(Event, Debug, Clone)]
pub struct LayoutSaveRequested;

#[derive(Event, Debug, Clone)]
pub struct LayoutSaveAsRequested;

#[derive(Event, Debug, Clone)]
pub struct LayoutLoadDialogRequested;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutLoadMode {
    UserJsonOpen,
    ApplySidecarForPy,
}

#[derive(Event, Debug, Clone)]
pub struct LayoutLoadRequested {
    pub path: PathBuf,
    pub mode: LayoutLoadMode,
}

/// 非同期ファイルダイアログ（rfd::AsyncFileDialog）の種別（Issue #17）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileDialogKind {
    Load,
    Save,
    SaveAs,
}

/// 単一のファイルダイアログ pending task（Issue #17）。
/// macOS でメインスレッドをブロックしないよう、ダイアログをワーカーで回し
/// 毎フレーム poll する。
///
/// load / save / save-as を **1 本に統一**して相互排他にすることで、
/// 旧同期 API のモーダル性（同時に 1 つしかダイアログが開けない）を復元する。
/// これがないと、ダイアログ表示中に別種ダイアログを起動でき、完了順に
/// `StrategyBuffer` / `ScenarioReadTarget` / cache / ファイルを競合して書き換えうる。
#[derive(Resource, Default)]
pub struct PendingFileDialog {
    pub task: Option<Task<Option<PathBuf>>>,
    pub kind: Option<FileDialogKind>,
    /// テスト用 seam（Issue #21）：ダイアログ結果を非同期 task を介さず直接注入する。
    /// 外側 `Some` = 注入済みで未消費、内側 `Option<PathBuf>` = 選択パス or キャンセル。
    /// production では誰も `inject_resolved` を呼ばないため常に `None`。
    pub resolved: Option<Option<PathBuf>>,
}

impl PendingFileDialog {
    /// いずれかのダイアログが表示中か。新規ダイアログ起動の可否判定に使う。
    pub fn is_active(&self) -> bool {
        self.task.is_some() || self.resolved.is_some()
    }

    /// 新規ダイアログを開始する（呼び出し側で `is_active()` を確認済みの前提）。
    fn begin(&mut self, kind: FileDialogKind, task: Task<Option<PathBuf>>) {
        self.task = Some(task);
        self.kind = Some(kind);
    }

    /// 指定 kind のダイアログを poll する。
    /// - `kind` が一致しない（別種が走っている / 何も走っていない）→ `None`
    /// - 一致して未完了 → `None`
    /// - 一致して完了 → guard を解放し `Some(結果)`（内側は選択パス or キャンセル時 None）
    fn poll_take(&mut self, kind: FileDialogKind) -> Option<Option<PathBuf>> {
        if self.kind != Some(kind) {
            return None;
        }
        // テスト seam（Issue #21）：注入済み結果があれば task より優先して消費する。
        if let Some(result) = self.resolved.take() {
            self.kind = None;
            return Some(result);
        }
        let task = self.task.as_mut()?;
        let result = future::block_on(future::poll_once(task))?;
        self.task = None;
        self.kind = None;
        Some(result)
    }

    /// テスト用 seam（Issue #21）：非同期 task を回さずダイアログ結果を直接注入する。
    /// `kind` をセットして `poll_take(kind)` が次フレームで `Some(path)` を返すようにする。
    pub fn inject_resolved(&mut self, kind: FileDialogKind, path: Option<PathBuf>) {
        self.task = None;
        self.kind = Some(kind);
        self.resolved = Some(path);
    }
}

#[derive(Event, Debug, Clone)]
pub struct CacheRestoreRequested {
    pub layout: SidecarLayout,
}

/// issue #25 / Phase 7.5: restore 時に layout JSON 由来の WindowLayout を spawn/pending
/// 投入する前に弾く。`restore_driver()==ScenarioInstruments`（Chart / Order）は
/// scenario 所有で layout-persist 対象外なので skip する。
#[inline]
fn is_non_persisted_layout_entry(win_layout: &WindowLayout) -> bool {
    if win_layout.kind.restore_driver() == PanelRestoreDriver::ScenarioInstruments {
        warn!(
            "layout: skipping scenario-owned PanelKind::{:?} entry (pos={:?}, size={:?}); \
             not layout-persisted (issue #25)",
            win_layout.kind, win_layout.position, win_layout.size
        );
        true
    } else {
        false
    }
}

/// world-space sprite window（`build_layout` 用）の read-only クエリ型。
/// `Without<ScreenWindowRoot>` で screen-space window を明示的に除外し、`ScreenPanelQuery` と
/// アーキタイプ非交差にする（screen root が将来 `Sprite` を得ても二重カウントしない）。
type WorldPanelQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PanelKind,
        Option<&'static StrategyEditorId>,
        Option<&'static StrategyEditorModeHidden>,
        &'static Transform,
        &'static Sprite,
        &'static Visibility,
    ),
    (
        With<WindowRoot>,
        Without<LayoutExcluded>,
        Without<ScreenWindowRoot>,
    ),
>;

/// screen-space `Node` window（Strategy Editor / Startup、ADR 0003）の read-only クエリ型。
/// world-space と異なり geometry は `Node`(left/top/width/height)・z は `GlobalZIndex`。
type ScreenPanelQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PanelKind,
        Option<&'static StrategyEditorId>,
        Option<&'static StrategyEditorModeHidden>,
        &'static Node,
        &'static GlobalZIndex,
        &'static Visibility,
    ),
    (
        With<WindowRoot>,
        With<ScreenWindowRoot>,
        Without<LayoutExcluded>,
    ),
>;

/// ECS 状態から `SidecarLayout` を組み立てる。
///
/// `preserve_scenario_json` に `Some(path)` を渡すと、その `.json` パスから
/// 既存 `scenario` キーを回収して新 layout に含める（F1 対応）。
/// `None` を渡すと `scenario` は `None` のままになる。
///
/// world-space sprite window（`panels`）と screen-space `Node` window（`screen_panels`、
/// ADR 0003 の Strategy Editor / Startup）の両方を 1 つの `windows` 配列に収集する。
#[allow(clippy::type_complexity)]
fn build_layout(
    panels: &WorldPanelQuery<'_, '_>,
    screen_panels: &ScreenPanelQuery<'_, '_>,
    camera: &Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: &StrategyBuffer,
    preserve_scenario_json: Option<&std::path::Path>,
) -> SidecarLayout {
    let viewport = camera
        .single()
        .map(|(cam_tf, proj)| ViewportState {
            pan_x: cam_tf.translation.x,
            pan_y: cam_tf.translation.y,
            zoom: viewport_zoom(proj),
        })
        .unwrap_or_default();

    // issue #31: LiveManual 中の Strategy Editor は `apply_strategy_editor_mode_visibility_system`
    // が live `Visibility` を一時的に `Hidden` へ固定している。そのまま保存すると「layout の
    // visible は権威」という不変条件を破り、Manual 中の autosave / 明示 Save が editor を
    // `visible:false` に焼き込んで Replay でも消えたままになる。退避マーカーがあれば、その退避値
    // （＝本来の意図）を保存する。
    fn saved_visible(mode_hidden: Option<&StrategyEditorModeHidden>, vis: &Visibility) -> bool {
        match mode_hidden {
            Some(StrategyEditorModeHidden(saved)) => !matches!(saved, Visibility::Hidden),
            None => !matches!(vis, Visibility::Hidden),
        }
    }

    let mut windows: Vec<WindowLayout> = panels
        .iter()
        .filter(|(kind, ..)| kind.restore_driver() != PanelRestoreDriver::ScenarioInstruments)
        .map(|(kind, id, mode_hidden, tf, sprite, vis)| WindowLayout {
            kind: *kind,
            visible: saved_visible(mode_hidden, vis),
            position: [tf.translation.x, tf.translation.y],
            size: sprite.custom_size.unwrap_or(Vec2::ZERO).to_array(),
            z: tf.translation.z,
            region_key: id.map(|i| i.region_key.clone()),
        })
        .collect();

    // ADR 0003: screen-space window（Node geometry）を同じ windows 配列へ追記する。
    // position = Node の left/top、size = Node の width/height、z = GlobalZIndex（i32→f32）。
    windows.extend(
        screen_panels
            .iter()
            .filter(|(kind, ..)| kind.restore_driver() != PanelRestoreDriver::ScenarioInstruments)
            .map(|(kind, id, mode_hidden, node, z, vis)| WindowLayout {
                kind: *kind,
                visible: saved_visible(mode_hidden, vis),
                position: [px_of(node.left), px_of(node.top)],
                size: [px_of(node.width), px_of(node.height)],
                z: z.0 as f32,
                region_key: id.map(|i| i.region_key.clone()),
            }),
    );

    let strategy_path = buffer
        .original_path
        .as_ref()
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    // 既存サイドカーから scenario キーを回収して merge（F1: save 時に scenario が消えるのを防ぐ）
    let scenario = preserve_scenario_json
        .filter(|p| p.exists())
        .and_then(|p| read_json_with_bom_strip(&p).ok())
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

/// 計画書 KC4-c: 明示 Save 専用 layout 構築。
/// - `cache_sidecar` を preserve scenario の第一候補とする。
/// - cache sidecar が無い / 読めない場合、`fallback_original_json` を二次候補にする
///   (通常 autosave / window close からはこの fallback を呼ばないこと)。
/// - registry.editable == true なら preserved scenario.instruments を registry で必ず上書きする。
///   scenario object が無い場合は `ScenarioMetadata` から最小 v2 scenario を構築する。
///   ただし start/end/granularity/initial_cash のいずれかが欠ける場合は
///   壊れた scenario を作らないため None を返す（呼び出し側で skip 判断）。
/// - registry.editable == false の場合は scenario 形状を一切変更しない。
#[allow(clippy::type_complexity)]
fn build_layout_for_explicit_save(
    panels: &WorldPanelQuery<'_, '_>,
    screen_panels: &ScreenPanelQuery<'_, '_>,
    camera: &Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: &StrategyBuffer,
    registry: &crate::ui::components::InstrumentRegistry,
    scenario_meta: &crate::ui::components::ScenarioMetadata,
    cache_sidecar: Option<&std::path::Path>,
    fallback_original_json: Option<&std::path::Path>,
) -> Option<SidecarLayout> {
    // preserve source: cache 第一、fallback 第二
    let preserve_from: Option<&std::path::Path> = cache_sidecar.or(fallback_original_json);
    let mut layout = build_layout(panels, screen_panels, camera, buffer, preserve_from);

    if !registry.editable {
        // instruments_ref などは scenario 形状を壊さない
        return Some(layout);
    }

    // editable == true: scenario.instruments を registry で必ず上書き
    let instruments_json = serde_json::Value::Array(
        registry
            .as_slice()
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect(),
    );

    match layout.scenario.as_mut() {
        Some(serde_json::Value::Object(map)) => {
            map.insert("instruments".to_string(), instruments_json);
        }
        _ => {
            // 既存 scenario が無い: ScenarioMetadata から最小 v2 を構築
            let (Some(start), Some(end), Some(granularity), Some(initial_cash)) = (
                scenario_meta.start.as_ref(),
                scenario_meta.end.as_ref(),
                scenario_meta.granularity.as_ref(),
                scenario_meta.initial_cash,
            ) else {
                error!(
                    "explicit save: scenario required fields missing (start/end/granularity/initial_cash); skipping save"
                );
                return None;
            };
            let mut map = serde_json::Map::new();
            map.insert(
                "schema_version".to_string(),
                serde_json::Value::Number(serde_json::Number::from(
                    scenario_meta.schema_version.unwrap_or(2),
                )),
            );
            map.insert("instruments".to_string(), instruments_json);
            map.insert(
                "start".to_string(),
                serde_json::Value::String(start.clone()),
            );
            map.insert("end".to_string(), serde_json::Value::String(end.clone()));
            map.insert(
                "granularity".to_string(),
                serde_json::Value::String(granularity.clone()),
            );
            map.insert(
                "initial_cash".to_string(),
                serde_json::Value::Number(serde_json::Number::from(initial_cash)),
            );
            layout.scenario = Some(serde_json::Value::Object(map));
        }
    }

    Some(layout)
}

fn save_layout_to(path: &PathBuf, layout: &SidecarLayout) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(layout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// UTF-8 BOM (0xEF 0xBB 0xBF) を読み飛ばしてから JSON parse する。
/// PowerShell の `Out-File`、Notepad、`Set-Content -Encoding UTF8` などは
/// BOM 付きで書き出すため、それらで作られた sidecar JSON を寛容に読む。
pub(crate) fn read_json_with_bom_strip(path: &std::path::Path) -> std::io::Result<String> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.trim_start_matches('\u{FEFF}').to_string())
}

pub(crate) fn sidecar_has_windows(path: &std::path::Path) -> bool {
    if !path.exists() {
        return false;
    }
    read_json_with_bom_strip(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("windows").cloned())
        .map(|w| !w.is_null())
        .unwrap_or(false)
}

fn load_layout_from(path: &PathBuf) -> std::io::Result<SidecarLayout> {
    let text = read_json_with_bom_strip(path)?;
    serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// cache restore で `layout.windows` 内に既に non-legacy な
/// `PanelKind::StrategyEditor` エントリが存在するかを判定する。
/// 存在すれば既存の windows ループが spawn を担当するため fallback は不要。
fn cache_restore_has_strategy_editor_window(windows: Option<&Vec<WindowLayout>>) -> bool {
    let Some(list) = windows else {
        return false;
    };
    list.iter()
        .any(|w| w.kind == PanelKind::StrategyEditor && !is_non_persisted_layout_entry(w))
}

/// cache restore fallback: `windows` 内に non-legacy StrategyEditor が無いとき、
/// `fragments` の各 key について PanelSpawnRequested を生成して返す。
/// `dedupe` に既に含まれる region_key はスキップする（caller が後で extend する）。
/// caller 側で dedupe set への insert と event writer への送出を担当。
fn compute_cache_restore_fallback_spawns(
    windows: Option<&Vec<WindowLayout>>,
    fragments: &[(String, String)],
    dedupe: &std::collections::HashSet<String>,
) -> Vec<PanelSpawnRequested> {
    if cache_restore_has_strategy_editor_window(windows) {
        return Vec::new();
    }
    fragments
        .iter()
        .filter(|(key, _)| !dedupe.contains(key))
        .map(|(key, _)| PanelSpawnRequested {
            kind: PanelKind::StrategyEditor,
            source: PanelSpawnSource::LayoutLoad,
            strategy_spec: Some(StrategyEditorSpawnSpec {
                region_key: Some(key.clone()),
                source: None,
                layout_source: PanelSpawnSource::LayoutLoad,
            }),
        })
        .collect()
}

pub fn apply_cache_restore_system(
    mut events: EventReader<CacheRestoreRequested>,
    mut buffer: ResMut<StrategyBuffer>,
    mut allocator: ResMut<RegionKeyAllocator>,
    mut pending_fragments: ResMut<PendingStrategyFragments>,
    mut camera: Query<
        (&mut Transform, &mut Projection),
        (With<Camera2d>, Without<WindowRoot>),
    >,
    mut pending: ResMut<PendingLayoutApply>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    mut scenario_target: ResMut<ScenarioReadTarget>, // ← ADD
) {
    for event in events.read() {
        let Some((cache_json, cache_py)) = cache_state_paths() else {
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
        // §3a: 起動 truth source は cache sidecar。元 sidecar は読まない。
        scenario_target.0 = Some(cache_json.clone());
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
            (&event.layout.viewport, camera.single_mut())
        {
            cam_tf.translation.x = vp.pan_x;
            cam_tf.translation.y = vp.pan_y;
            set_viewport_zoom(&mut proj, vp.zoom);
        }

        if let Some(win_layouts) = &event.layout.windows {
            for win_layout in win_layouts {
                if is_non_persisted_layout_entry(win_layout) {
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

        let dedupe_keys = pending
            .spawn_requested
            .iter()
            .filter(|(kind, _)| *kind == PanelKind::StrategyEditor)
            .filter_map(|(_, rk)| rk.clone())
            .collect::<std::collections::HashSet<String>>();
        let fallback_spawns = compute_cache_restore_fallback_spawns(
            event.layout.windows.as_ref(),
            &outcome.fragments,
            &dedupe_keys,
        );
        for req in fallback_spawns {
            let region_key = req
                .strategy_spec
                .as_ref()
                .and_then(|spec| spec.region_key.clone());
            if pending
                .spawn_requested
                .insert((PanelKind::StrategyEditor, region_key))
            {
                spawn_ev.send(req);
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

/// 保存パス確定後の共通処理（build → JSON 保存 → .py 保存 → cache sync）。
/// 同期 handler / 非同期 poll の両方から呼ぶため副作用のみのフリー関数として抽出。
/// 戻り値: 保存を中断（呼び出し側で次イベントへ）すべきなら false。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn finish_layout_save(
    json_path: &PathBuf,
    py_path: &PathBuf,
    was_new: bool,
    panels: &WorldPanelQuery<'_, '_>,
    screen_panels: &ScreenPanelQuery<'_, '_>,
    camera: &Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: &mut StrategyBuffer,
    fragments_q: &mut Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    strategy_auto_save: &mut StrategyAutoSaveState,
    registry: &crate::ui::components::InstrumentRegistry,
    paths: &crate::ui::components::ScenarioWritebackPaths,
    scenario: &crate::ui::components::ScenarioMetadata,
    scenario_target: &mut ScenarioReadTarget,
) -> bool {
    // 計画書 KC4: 明示 Save の前に cache 側だけ最新化する。
    // registry.editable == false (instruments_ref) 時はスキップ。flush 失敗しても
    // Save 自体は継続する（cache が壊れていても原本へは保存できるべき）。
    // Issue #17 finding #3: 非同期ダイアログでは pre-flush と write の間にフレームが
    // 挟まり state が drift しうるため、pre-flush は write 直前のここ（finish）で行う。
    if registry.editable {
        if let Err(e) = crate::ui::components::flush_sidecars_now(
            registry.as_slice(),
            None,
            paths.cache_sidecar.as_deref(),
        ) {
            warn!("Save: pre-flush to cache failed (continuing save): {}", e);
        }
    }

    let scenario_json = scenario_target.0.clone();
    let fallback_json = buffer
        .original_path
        .as_ref()
        .map(|p| p.with_extension("json"));
    let layout = match build_layout_for_explicit_save(
        panels,
        screen_panels,
        camera,
        &*buffer,
        registry,
        scenario,
        scenario_json.as_deref(),
        fallback_json.as_deref(),
    ) {
        Some(l) => l,
        None => {
            // helper が None を返した = required scenario fields 欠落。Save をスキップ。
            if was_new {
                buffer.original_path = None;
                buffer.cache_path = None;
                scenario_target.0 = None;
            }
            return false;
        }
    };
    match save_layout_to(json_path, &layout) {
        Ok(()) => {
            info!("layout saved to {:?}", json_path);
            scenario_target.0 = Some(json_path.clone());
        }
        Err(e) => {
            error!("layout save failed: {e}");
            // ロールバック: original_path を None に戻す（初回 save の場合）
            if was_new {
                buffer.original_path = None;
                buffer.cache_path = None;
                scenario_target.0 = None;
            }
            return false;
        }
    }

    let mut items: Vec<(String, String)> = fragments_q
        .iter()
        .map(|(id, frag)| (id.region_key.clone(), frag.source.clone()))
        .collect();
    if !items.is_empty() {
        items.sort_by(|a, b| a.0.cmp(&b.0));
        let merged = merge_fragments(&items);
        match std::fs::write(py_path, &merged) {
            Ok(()) => {
                info!("strategy .py saved to {:?}", py_path);
                match sync_to_cache(py_path) {
                    Ok(()) => {
                        for (_, mut frag) in fragments_q.iter_mut() {
                            frag.dirty = false;
                        }
                        strategy_auto_save.dirty = false;
                        strategy_auto_save.last_change = None;
                        buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py);
                    }
                    Err(e) => {
                        error!("failed to sync saved strategy to cache: {e}");
                        buffer.cache_path = None;
                    }
                }
            }
            Err(e) => {
                error!("strategy .py save failed: {e}");
                if was_new {
                    buffer.original_path = None;
                    buffer.cache_path = None;
                    scenario_target.0 = None;
                }
            }
        }
    }
    true
}

#[allow(clippy::type_complexity)]
pub fn handle_save_layout_system(
    mut events: EventReader<LayoutSaveRequested>,
    panels: WorldPanelQuery<'_, '_>,
    screen_panels: ScreenPanelQuery<'_, '_>,
    camera: Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
    registry: Res<crate::ui::components::InstrumentRegistry>,
    paths: Res<crate::ui::components::ScenarioWritebackPaths>,
    scenario: Res<crate::ui::components::ScenarioMetadata>,
    mut scenario_target: ResMut<ScenarioReadTarget>,
    mut save_as_writer: EventWriter<LayoutSaveAsRequested>,
) {
    for _ in events.read() {
        let was_new = buffer.original_path.is_none();

        // 初回保存（original_path 無し）は Save As 相当のダイアログが必要。
        // ここでは直接 rfd を起動せず、Save As フローへ委譲する（案A, Issue #21）。
        // 多重起動防止 guard は委譲先 handle_save_as_layout_system が持つため不要。
        if buffer.original_path.is_none() {
            save_as_writer.send(LayoutSaveAsRequested);
            continue;
        }
        let orig = buffer.original_path.as_ref().unwrap();

        // 既存パスあり = ダイアログ不要の同期保存。
        // pre-flush は finish_layout_save の冒頭（write 直前）で行う（Issue #17 finding #3）。
        let json_path = orig.with_extension("json");
        let py_path = json_path.with_extension("py");

        if !finish_layout_save(
            &json_path,
            &py_path,
            was_new,
            &panels,
            &screen_panels,
            &camera,
            &mut buffer,
            &mut fragments_q,
            &mut strategy_auto_save,
            &registry,
            &paths,
            &scenario,
            &mut scenario_target,
        ) {
            continue;
        }
    }
}

fn handle_save_as_layout_system(
    mut events: EventReader<LayoutSaveAsRequested>,
    mut pending: ResMut<PendingFileDialog>,
) {
    for _ in events.read() {
        if pending.is_active() {
            continue; // 多重起動防止: 何らかのダイアログ表示中（モーダル相当）
        }
        // pre-flush は poll_save_as_dialog_system の write 直前で行う（Issue #17 finding #3）。
        let task = AsyncComputeTaskPool::get().spawn(async move {
            rfd::AsyncFileDialog::new()
                .add_filter("Layout JSON", &["json"])
                .save_file()
                .await
                .map(|h| h.path().to_path_buf())
        });
        pending.begin(FileDialogKind::SaveAs, task);
    }
}

fn handle_load_dialog_system(
    mut events: EventReader<LayoutLoadDialogRequested>,
    mut pending: ResMut<PendingFileDialog>,
) {
    for _ in events.read() {
        if pending.is_active() {
            continue; // 多重起動防止: 何らかのダイアログ表示中（モーダル相当）
        }
        let task = AsyncComputeTaskPool::get().spawn(async move {
            rfd::AsyncFileDialog::new()
                .add_filter("Layout JSON", &["json"])
                .pick_file()
                .await
                .map(|h| h.path().to_path_buf())
        });
        pending.begin(FileDialogKind::Load, task);
    }
}

fn poll_load_dialog_system(
    mut pending: ResMut<PendingFileDialog>,
    mut writer: EventWriter<LayoutLoadRequested>,
) {
    let Some(result) = pending.poll_take(FileDialogKind::Load) else {
        return; // 別種 / 未完了
    };
    match result {
        Some(path) => {
            writer.send(LayoutLoadRequested {
                path,
                mode: LayoutLoadMode::UserJsonOpen,
            });
        }
        None => info!("layout load cancelled: no file selected"),
    }
}

#[allow(clippy::type_complexity)]
pub fn poll_save_as_dialog_system(
    mut pending: ResMut<PendingFileDialog>,
    panels: WorldPanelQuery<'_, '_>,
    screen_panels: ScreenPanelQuery<'_, '_>,
    camera: Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
    registry: Res<crate::ui::components::InstrumentRegistry>,
    paths: Res<crate::ui::components::ScenarioWritebackPaths>,
    scenario: Res<crate::ui::components::ScenarioMetadata>,
    mut writeback: ResMut<crate::ui::components::ScenarioInstrumentsWritebackState>,
    mut scenario_target: ResMut<crate::ui::components::ScenarioReadTarget>,
) {
    let Some(result) = pending.poll_take(FileDialogKind::SaveAs) else {
        return; // 別種 / 未完了
    };
    let json_path = match result {
        Some(p) => p,
        None => {
            info!("layout save-as cancelled: no path selected");
            return;
        }
    };

    // pre-flush（write 直前に cache を最新化、Issue #17 finding #3）。
    // registry.editable == false 時はスキップ。flush 失敗しても Save As 自体は継続。
    if registry.editable {
        if let Err(e) = crate::ui::components::flush_sidecars_now(
            registry.as_slice(),
            None,
            paths.cache_sidecar.as_deref(),
        ) {
            warn!("Save As: pre-flush to cache failed (continuing save): {}", e);
        }
    }

    let py_path = json_path.with_extension("py");

    // Fix(High): buffer を先に新パスへ更新 → build_layout の strategy_path が正しくなる
    let old_original = buffer.original_path.clone();
    let old_cache = buffer.cache_path.clone();
    let old_scenario_target = scenario_target.0.clone();
    buffer.original_path = Some(py_path.clone());
    buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py);

    // preserve_scenario_from: 現在 open 中の scenario_target を第一候補、
    // 無ければ Save As 限定で old_original_path.with_extension("json") に fallback。
    let scenario_json = old_scenario_target.clone();
    let old_original_json = old_original.as_ref().map(|p| p.with_extension("json"));
    let layout = match build_layout_for_explicit_save(
        &panels,
        &screen_panels,
        &camera,
        &*buffer,
        &registry,
        &scenario,
        scenario_json.as_deref(),
        old_original_json.as_deref(),
    ) {
        Some(l) => l,
        None => {
            // required scenario fields 欠落 → Save As skip + buffer ロールバック
            buffer.original_path = old_original;
            buffer.cache_path = old_cache;
            return;
        }
    };
    match save_layout_to(&json_path, &layout) {
        Ok(()) => {
            info!("layout saved-as to {:?}", json_path);
            scenario_target.0 = Some(json_path.clone());
        }
        Err(e) => {
            error!("layout save-as failed: {e}");
            // ロールバック
            buffer.original_path = old_original;
            buffer.cache_path = old_cache;
            return;
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
                match sync_to_cache(&py_path) {
                    Ok(()) => {
                        crate::ui::components::bump_writeback_for_save_as(&mut writeback);
                        for (_, mut frag) in fragments_q.iter_mut() {
                            frag.dirty = false;
                        }
                        strategy_auto_save.dirty = false;
                        strategy_auto_save.last_change = None;
                        buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py);
                    }
                    Err(e) => {
                        error!("failed to sync saved-as strategy to cache: {e}");
                        buffer.cache_path = None;
                    }
                }
            }
            Err(e) => {
                // Fix(Medium): .py 保存失敗時は buffer を元に戻す
                error!("strategy .py save-as failed: {e}");
                buffer.original_path = old_original;
                buffer.cache_path = old_cache;
                scenario_target.0 = old_scenario_target.clone();
            }
        }
    }
}

/// save（初回保存）ダイアログの非同期 poll（Issue #17）。
/// None 経路は必ず初回保存なので was_new = true 固定で finish_layout_save に委譲する。
#[allow(clippy::type_complexity)]
fn poll_save_dialog_system(
    mut pending: ResMut<PendingFileDialog>,
    panels: WorldPanelQuery<'_, '_>,
    screen_panels: ScreenPanelQuery<'_, '_>,
    camera: Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
    registry: Res<crate::ui::components::InstrumentRegistry>,
    paths: Res<crate::ui::components::ScenarioWritebackPaths>,
    scenario: Res<crate::ui::components::ScenarioMetadata>,
    mut scenario_target: ResMut<ScenarioReadTarget>,
) {
    let Some(result) = pending.poll_take(FileDialogKind::Save) else {
        return; // 別種 / 未完了
    };
    let json_path = match result {
        Some(p) => p,
        None => {
            info!("layout save cancelled: no path selected");
            return;
        }
    };

    let py_path = json_path.with_extension("py");

    // Save ダイアログはユーザーがパスを明示選択した結果。常にこのパスへ保存する。
    // 単一モーダル guard（PendingFileDialog）によりダイアログ表示中は他のダイアログを
    // 起動できないため、original_path は None のまま完了する（was_new = true は有効）。
    // build_layout の前に original_path を新パスへ更新する。
    buffer.original_path = Some(py_path.clone());
    buffer.cache_path = cache_state_paths().map(|(_, cache_py)| cache_py);

    if !finish_layout_save(
        &json_path,
        &py_path,
        true,
        &panels,
        &screen_panels,
        &camera,
        &mut buffer,
        &mut fragments_q,
        &mut strategy_auto_save,
        &registry,
        &paths,
        &scenario,
        &mut scenario_target,
    ) {
        return;
    }
}

/// screen-space window（`ScreenWindowRoot`）へ保存 geometry を復元する（ADR 0003）。
/// - position → `Node.left`/`Node.top`、z → `GlobalZIndex`。
/// - Startup は size・可視性を復元しない（size は窓側定数が正、可視性は `ExecutionMode` 所有 [M9]）。
/// - Strategy Editor は size・可視性も layout が権威。issue #31 の退避マーカーがあれば、その
///   マーカーへ intent を書く（Manual 中の layout load でマーカーが陳腐化するのを防ぐ）。
fn restore_screen_window_geometry(
    win_layout: &WindowLayout,
    kind: &PanelKind,
    node: &mut Node,
    z: &mut GlobalZIndex,
    vis: &mut Visibility,
    mode_hidden: Option<Mut<StrategyEditorModeHidden>>,
) {
    node.left = Val::Px(win_layout.position[0]);
    node.top = Val::Px(win_layout.position[1]);
    z.0 = win_layout.z as i32;
    if *kind != PanelKind::Startup {
        node.width = Val::Px(win_layout.size[0]);
        node.height = Val::Px(win_layout.size[1]);
        let intended = if win_layout.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some(mut marker) = mode_hidden {
            marker.0 = intended;
        } else {
            *vis = intended;
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
// pub: headless integration test（tests/e2e/flows/i5_*）が file-open → spawn の
// seam を駆動するため。本番の登録は LayoutPersistencePlugin 内のまま。
pub fn apply_layout_system(
    mut commands: Commands,
    mut events: EventReader<LayoutLoadRequested>,
    // world-space sprite window（geometry = Transform/Sprite）。screen window と &mut Visibility が
    // 競合しないよう Without<ScreenWindowRoot> で明示的に分離する。
    mut panels: Query<
        (
            Entity,
            &PanelKind,
            Option<&StrategyEditorId>,
            &mut Transform,
            &mut Sprite,
            &mut Visibility,
            Option<&mut StrategyEditorModeHidden>,
        ),
        (
            With<WindowRoot>,
            Without<LayoutExcluded>,
            Without<ScreenWindowRoot>,
        ),
    >,
    // screen-space Node window（geometry = Node left/top/width/height、z = GlobalZIndex、ADR 0003）。
    mut screen_panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &mut Node,
            &mut GlobalZIndex,
            &mut Visibility,
            Option<&mut StrategyEditorModeHidden>,
        ),
        (
            With<WindowRoot>,
            With<ScreenWindowRoot>,
            Without<LayoutExcluded>,
        ),
    >,
    mut camera: Query<
        (&mut Transform, &mut Projection),
        (With<Camera2d>, Without<WindowRoot>),
    >,
    mut wm: ResMut<WindowManager>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    mut pending: ResMut<PendingLayoutApply>,
    mut load_ev: EventWriter<StrategyFileLoadRequested>,
    mut pending_fragments: ResMut<PendingStrategyFragments>,
    // ワンショット loopback 抑制: 直近で scenario-only Open → sibling .py 発火 →
    // handler が同じ JSON を再発火、までの 1 サイクルだけスキップする。
    // pending_fragments.loaded_for_path のような恒久的状態に基づくと、
    // 「同じ JSON を後から再 Open」したケースまで抑制されてしまうため。
    mut pending_loopback: Local<Option<PathBuf>>,
    mut scenario_target: ResMut<ScenarioReadTarget>,
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
            // sibling .py が無い scenario-only JSON: UI に scenario を反映するため
            // ScenarioReadTarget だけ更新し、layout 側は素通りで終わらせる。
            info!(
                "scenario-only JSON {:?} opened with no sibling; updating scenario only",
                event.path
            );
            scenario_target.0 = Some(event.path.clone());
            continue;
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
                let should_load = match event.mode {
                    LayoutLoadMode::UserJsonOpen => true,
                    LayoutLoadMode::ApplySidecarForPy => {
                        // `.py` UserOpen 後の sidecar 適用では、ユーザーが明示選択した
                        // `.py` を source of truth とする。sidecar の strategy_path が
                        // 違っていても読み替えない。
                        match &pending_fragments.loaded_for_path {
                            Some(p) if p == &path => {
                                debug!(
                                    "apply_layout_system: skipping strategy_path reload \
                                     (already loaded: {:?})",
                                    path
                                );
                                false
                            }
                            Some(user_path) => {
                                warn!(
                                    "apply_layout_system: sidecar strategy_path {:?} \
                                     differs from user-selected {:?}; ignoring sidecar path",
                                    path, user_path
                                );
                                false
                            }
                            _ => true,
                        }
                    }
                };
                if should_load {
                    // 連続 Load / 失敗後リトライに備え、前回 deferred apply の残骸を
                    // 一貫して初期化する。直後に load_ev / pending.windows.extend /
                    // waiting_for_strategy = true が再設定するので、ここでは
                    // 「捨てる」ことだけに専念する。
                    pending_fragments.by_region_key.clear();
                    pending_fragments.loaded_for_path = None;
                    pending.windows.clear();
                    pending.spawn_requested.clear();
                    pending.waiting_for_strategy = false;

                    load_ev.send(StrategyFileLoadRequested {
                        path,
                        mode: StrategyLoadMode::LayoutRestore,
                    });
                    // ウィンドウ spawn をキューして翌フレームまで defer する
                    if let Some(win_layouts) = &layout.windows {
                        pending.windows.extend(win_layouts.iter().cloned());
                        pending.waiting_for_strategy = true;
                    }
                    // カメラは同フレーム内で適用可能
                    if let (Some(vp), Ok((mut cam_tf, mut proj))) =
                        (&layout.viewport, camera.single_mut())
                    {
                        cam_tf.translation.x = vp.pan_x;
                        cam_tf.translation.y = vp.pan_y;
                        set_viewport_zoom(&mut proj, vp.zoom);
                    }
                    info!(
                        "layout apply deferred (waiting for strategy fragments): {:?}",
                        event.path
                    );
                    continue;
                }
            } else {
                warn!("layout load: strategy_path {:?} not found, skipping", path);
                // UserJsonOpen で存在しない strategy_path（例: 別 OS の絶対パス）を開いたとき、
                // 前セッションの stale な fragments が editor に残らないよう破棄する。
                if event.mode == LayoutLoadMode::UserJsonOpen {
                    pending_fragments.by_region_key.clear();
                    pending_fragments.loaded_for_path = None;
                }
            }
        }

        // viewport: None → カメラを触らない（F10: scenario-only JSON で camera reset を防ぐ）
        if let (Some(vp), Ok((mut cam_tf, mut proj))) = (&layout.viewport, camera.single_mut())
        {
            cam_tf.translation.x = vp.pan_x;
            cam_tf.translation.y = vp.pan_y;
            set_viewport_zoom(&mut proj, vp.zoom);
        }

        // windows: None → despawn/spawn を一切しない（F10: 既存パネルを消さない）
        // windows: Some(list) → 既存ロジック通り（list に無いパネルを despawn）
        if let Some(win_layouts) = &layout.windows {
            let mut new_max_z = wm.max_z;
            for win_layout in win_layouts {
                if is_non_persisted_layout_entry(win_layout) {
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
                let found = panels.iter_mut().find(|(_, kind, id, _, _, _, _)| {
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
                        // world-space miss → screen-space window（ScreenWindowRoot）を試す（ADR 0003）。
                        // Strategy Editor / Startup は screen window なので本番ではこちらで match する。
                        let screen_found = screen_panels.iter_mut().find(|(kind, id, ..)| {
                            if **kind != win_layout.kind {
                                return false;
                            }
                            match (win_layout.kind, want_key.as_deref(), id.as_ref()) {
                                (PanelKind::StrategyEditor, Some(k), Some(eid)) => {
                                    eid.region_key == k
                                }
                                (PanelKind::StrategyEditor, _, _) => false,
                                _ => true,
                            }
                        });
                        if let Some((kind, _, mut node, mut z, mut vis, mode_hidden)) = screen_found
                        {
                            restore_screen_window_geometry(
                                win_layout, kind, &mut node, &mut z, &mut vis, mode_hidden,
                            );
                            if win_layout.z > new_max_z {
                                new_max_z = win_layout.z;
                            }
                            continue;
                        }

                        // Startup は起動スケジュールで一度だけ spawn し、フィールドも
                        // そこで attach する。layout 経由で再 spawn するとフィールド無しの
                        // 壊れた窓になるため、layout からは spawn しない。
                        if win_layout.kind == PanelKind::Startup {
                            continue;
                        }
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
                    Some((_, kind, _, mut tf, mut sprite, mut vis, mode_hidden)) => {
                        tf.translation.x = win_layout.position[0];
                        tf.translation.y = win_layout.position[1];
                        tf.translation.z = win_layout.z;
                        // Startup は position/z のみ復元する。size と可視性は復元しない
                        // （可視性は ExecutionMode が所有し、size は窓側定数が正。古い
                        // layout の size を当てると窓幅が巻き戻り、子要素とズレる）。
                        if *kind != PanelKind::Startup {
                            sprite.custom_size = Some(Vec2::from_array(win_layout.size));
                            let intended = if win_layout.visible {
                                Visibility::Inherited
                            } else {
                                Visibility::Hidden
                            };
                            // issue #31: Manual 中の Strategy Editor は mode system が live
                            // Visibility を Hidden に固定している。layout が指定する本来の可視性は
                            // 退避マーカーへ書き、Manual を抜けたとき mode system がこの最新 intent へ
                            // 復元する（Manual 中の layout load でマーカーが陳腐化するのを防ぐ）。
                            if let Some(mut marker) = mode_hidden {
                                marker.0 = intended;
                            } else {
                                *vis = intended;
                            }
                        }
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
                .filter(|(_, kind, id, _, _, _, _)| {
                    // Startup は layout の windows リストに依存しない（起動時に一度だけ
                    // spawn・可視性は ExecutionMode が所有）。list に無くても despawn しない。
                    // pre-#14 sidecar など Startup を含まない layout でも消えないように。
                    if **kind == PanelKind::Startup {
                        return false;
                    }
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
                .map(|(entity, _, _, _, _, _, _)| entity)
                .collect();
            for entity in to_despawn {
                commands.entity(entity).despawn();
            }
        }

        info!("layout applied from {:?}", event.path);
    }
}

// pub: headless integration test（i5_*）が strategy_path 経由の deferred spawn を駆動するため。
pub fn apply_pending_layout_system(
    mut pending: ResMut<PendingLayoutApply>,
    mut panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &mut Transform,
            &mut Sprite,
            &mut Visibility,
            Option<&mut StrategyEditorModeHidden>,
        ),
        (
            With<WindowRoot>,
            Without<LayoutExcluded>,
            Without<ScreenWindowRoot>,
        ),
    >,
    // screen-space Node window（deferred spawn された Strategy Editor をここで geometry 復元する）。
    mut screen_panels: Query<
        (
            &PanelKind,
            Option<&StrategyEditorId>,
            &mut Node,
            &mut GlobalZIndex,
            &mut Visibility,
            Option<&mut StrategyEditorModeHidden>,
        ),
        (
            With<WindowRoot>,
            With<ScreenWindowRoot>,
            Without<LayoutExcluded>,
        ),
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
        if is_non_persisted_layout_entry(&win_layout) {
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
                // world-space miss → deferred spawn された screen-space window を試す（ADR 0003）。
                let screen_found = screen_panels.iter_mut().find(|(kind, id, ..)| {
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
                if let Some((kind, _, mut node, mut z, mut vis, mode_hidden)) = screen_found {
                    restore_screen_window_geometry(
                        &win_layout, kind, &mut node, &mut z, &mut vis, mode_hidden,
                    );
                    if win_layout.z > wm.max_z {
                        wm.max_z = win_layout.z;
                    }
                    continue;
                }

                // Startup は layout から spawn しない（フィールド attach は起動スケジュールのみ）。
                if win_layout.kind == PanelKind::Startup {
                    continue;
                }
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
            Some((kind, _, mut tf, mut sprite, mut vis, mode_hidden)) => {
                tf.translation.x = win_layout.position[0];
                tf.translation.y = win_layout.position[1];
                tf.translation.z = win_layout.z;
                // Startup は position/z のみ復元（size・可視性は復元しない）。
                if *kind != PanelKind::Startup {
                    sprite.custom_size = Some(Vec2::from_array(win_layout.size));
                    let intended = if win_layout.visible {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    // issue #31: Manual 中の Strategy Editor は退避マーカーへ intent を書く
                    // （mode system が live Visibility を Hidden に固定しているため）。
                    if let Some(mut marker) = mode_hidden {
                        marker.0 = intended;
                    } else {
                        *vis = intended;
                    }
                }
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
    panels: WorldPanelQuery<'_, '_>,
    screen_panels: ScreenPanelQuery<'_, '_>,
    camera: Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    mut buffer: ResMut<StrategyBuffer>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut strategy_auto_save: ResMut<StrategyAutoSaveState>,
    paths: Res<crate::ui::components::ScenarioWritebackPaths>,
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
        let layout = build_layout(
            &panels,
            &screen_panels,
            &camera,
            &*buffer,
            paths.cache_sidecar.as_deref(),
        );
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
    panels: WorldPanelQuery<'_, '_>,
    screen_panels: ScreenPanelQuery<'_, '_>,
    camera: Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
    buffer: Res<StrategyBuffer>,
    paths: Res<crate::ui::components::ScenarioWritebackPaths>,
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
    let layout = build_layout(
        &panels,
        &screen_panels,
        &camera,
        &*buffer,
        paths.cache_sidecar.as_deref(),
    );
    match save_layout_to(&cache_json, &layout) {
        Ok(()) => info!("debounced autosave → {:?}", cache_json),
        Err(e) => error!("debounced autosave failed: {e}"),
    }
    auto_save.dirty = false;
    auto_save.last_change = None;
}

// pub: headless integration test（i5_*）が Ctrl+O ジェスチャ→LayoutLoadDialogRequested を駆動するため。
pub fn layout_shortcut_system(
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
            .init_resource::<PendingFileDialog>()
            .init_resource::<AutoSaveState>()
            .add_event::<LayoutSaveRequested>()
            .add_event::<LayoutSaveAsRequested>()
            .add_event::<LayoutLoadDialogRequested>()
            .add_event::<LayoutLoadRequested>()
            .add_event::<CacheRestoreRequested>()
            .add_systems(
                Update,
                (
                    // issue #31: layout を直列化する save 系は mode system の前に走らせる。
                    // mode system は Manual 突入/新規 spawn 時に live Visibility を Hidden へ
                    // 強制しつつ退避マーカーを deferred Commands で insert するため、その後に
                    // save が走るとマーカー未反映の forced-Hidden を `visible:false` として焼き込む。
                    // 前に置けば「強制前の可視性（=本来の意図）」を読むので焼き込みを防げる。
                    (
                        handle_save_layout_system,
                        handle_save_as_layout_system,
                        poll_save_as_dialog_system,
                        poll_save_dialog_system,
                        debounced_autosave_system, // デバウンス自動保存
                    )
                        .before(
                            crate::ui::strategy_editor::apply_strategy_editor_mode_visibility_system,
                        ),
                    handle_load_dialog_system,
                    poll_load_dialog_system,
                    apply_cache_restore_system,
                    apply_layout_system,
                    // spawn dispatcher の後にも順序付け: Manual 中に layout が新規 spawn した
                    // StrategyEditor を同フレームで見つけ intended 可視性を確定させ、mode system が
                    // それを正しくマーカーへ捕捉できるようにする (マーカー陳腐化の解消)。
                    apply_pending_layout_system
                        .after(apply_layout_system)
                        .after(crate::ui::floating_window::panel_spawn_dispatcher_system),
                    layout_shortcut_system,
                ),
            )
            .add_systems(
                Update,
                save_layout_on_window_close
                    .before(crate::ui::strategy_editor::apply_strategy_editor_mode_visibility_system),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::PanelKind;

    #[test]
    fn cache_restore_has_strategy_editor_window_returns_false_for_empty_list() {
        let windows: Vec<WindowLayout> = vec![];
        assert!(
            !cache_restore_has_strategy_editor_window(Some(&windows)),
            "空 list では fallback が必要 → false を返すべき"
        );
    }

    #[test]
    fn cache_restore_has_strategy_editor_window_detects_non_legacy_entry() {
        let windows = vec![WindowLayout {
            kind: PanelKind::StrategyEditor,
            visible: true,
            position: [0.0, 0.0],
            size: [800.0, 600.0],
            z: 0.0,
            region_key: None,
        }];
        assert!(
            cache_restore_has_strategy_editor_window(Some(&windows)),
            "non-legacy StrategyEditor が存在すれば true（= fallback 不要）"
        );
    }

    #[test]
    fn compute_cache_restore_fallback_spawns_empty_windows_one_fragment() {
        let windows: Vec<WindowLayout> = vec![];
        let fragments = vec![("region_1".to_string(), "pass".to_string())];
        let dedupe = std::collections::HashSet::new();
        let out = compute_cache_restore_fallback_spawns(Some(&windows), &fragments, &dedupe);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, PanelKind::StrategyEditor);
        let spec = out[0]
            .strategy_spec
            .as_ref()
            .expect("strategy_spec must be Some");
        assert_eq!(spec.region_key.as_deref(), Some("region_1"));
        assert!(
            spec.source.is_none(),
            "source=None で dispatcher drain に委ねる"
        );
    }

    #[test]
    fn compute_cache_restore_fallback_spawns_skips_when_editor_window_exists() {
        let windows = vec![WindowLayout {
            kind: PanelKind::StrategyEditor,
            visible: true,
            position: [0.0, 0.0],
            size: [800.0, 600.0],
            z: 0.0,
            region_key: Some("region_1".to_string()),
        }];
        let fragments = vec![("region_1".to_string(), "pass".to_string())];
        let dedupe = std::collections::HashSet::new();
        let out = compute_cache_restore_fallback_spawns(Some(&windows), &fragments, &dedupe);
        assert!(
            out.is_empty(),
            "既存 StrategyEditor window があれば fallback は走らない"
        );
    }

    #[test]
    fn compute_cache_restore_fallback_spawns_windows_none_one_fragment() {
        let fragments = vec![("region_1".to_string(), "pass".to_string())];
        let dedupe = std::collections::HashSet::new();
        let out = compute_cache_restore_fallback_spawns(None, &fragments, &dedupe);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, PanelKind::StrategyEditor);
    }

    #[test]
    fn compute_cache_restore_fallback_spawns_non_editor_windows_one_fragment() {
        let windows = vec![WindowLayout {
            kind: PanelKind::Orders,
            visible: true,
            position: [0.0, 0.0],
            size: [200.0, 600.0],
            z: 0.0,
            region_key: None,
        }];
        let fragments = vec![("region_1".to_string(), "pass".to_string())];
        let dedupe = std::collections::HashSet::new();
        let out = compute_cache_restore_fallback_spawns(Some(&windows), &fragments, &dedupe);
        assert_eq!(out.len(), 1, "StrategyEditor 不在なら fallback が走る");
    }

    #[test]
    fn compute_cache_restore_fallback_spawns_skips_dedupe_keys() {
        let fragments = vec![
            ("region_1".to_string(), "pass".to_string()),
            ("region_2".to_string(), "pass".to_string()),
        ];
        let mut dedupe = std::collections::HashSet::new();
        dedupe.insert("region_1".to_string());
        let out = compute_cache_restore_fallback_spawns(None, &fragments, &dedupe);
        assert_eq!(out.len(), 1);
        let spec = out[0].strategy_spec.as_ref().unwrap();
        assert_eq!(spec.region_key.as_deref(), Some("region_2"));
    }

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

        // preserve_scenario_json に json_path を渡すと scenario が回収される
        let scenario = Some(json_path.as_path())
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
        assert!(is_non_persisted_layout_entry(&chart), "Chart entry must be skipped");
        assert!(
            !is_non_persisted_layout_entry(&orders),
            "non-Chart entries must pass through"
        );
    }

    #[test]
    fn legacy_order_window_layout_is_skipped() {
        let order = WindowLayout {
            kind: PanelKind::Order,
            visible: true,
            position: [10.0, 20.0],
            size: [400.0, 300.0],
            z: 1.0,
            region_key: None,
        };
        assert!(
            is_non_persisted_layout_entry(&order),
            "Order entry must be skipped on restore (issue #25 Slice 3)"
        );
    }

    #[test]
    fn build_layout_excludes_chart_instrument_roots() {
        use crate::ui::components::ChartInstrument;
        use bevy::ecs::system::SystemState;
        use bevy::prelude::*;

        let mut app = App::new();
        app.insert_resource(StrategyBuffer::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Chart,
            Transform::from_xyz(10.0, 20.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(400.0, 300.0)),
                ..default()
            },
            Visibility::Visible,
            ChartInstrument {
                instrument_id: "7203.TSE".to_string(),
            },
            LayoutExcluded,
        ));

        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Orders,
            Transform::from_xyz(0.0, 0.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(200.0, 150.0)),
                ..default()
            },
            Visibility::Visible,
        ));

        let mut state: SystemState<(
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Transform,
                    &Sprite,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    Without<LayoutExcluded>,
                    Without<ScreenWindowRoot>,
                ),
            >,
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Node,
                    &GlobalZIndex,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    With<ScreenWindowRoot>,
                    Without<LayoutExcluded>,
                ),
            >,
            Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
            Res<StrategyBuffer>,
        )> = SystemState::new(app.world_mut());

        let (panels, screen_panels, camera, buffer) = state.get(app.world());
        let layout = build_layout(&panels, &screen_panels, &camera, &*buffer, None);

        let windows = layout.windows.expect("windows must be Some");
        assert_eq!(windows.len(), 1, "ChartInstrument 付き root は除外される");
        assert_eq!(windows[0].kind, PanelKind::Orders);
    }

    /// issue #31 回帰: LiveManual 中に強制 `Hidden` された Strategy Editor を保存しても、
    /// 退避マーカーの「本来の可視性」が `visible` として書かれる（forced-Hidden を焼き込まない）。
    /// これが壊れると Manual 中の autosave / 明示 Save が editor を `visible:false` にし、
    /// Replay でも editor が消えたままになる（「layout visible は権威」不変条件の破壊）。
    #[test]
    fn build_layout_uses_mode_hidden_marker_visibility_for_strategy_editor() {
        use bevy::ecs::system::SystemState;
        use bevy::prelude::*;

        let mut app = App::new();
        app.insert_resource(StrategyBuffer::default());
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // (a) Manual で強制 Hidden だが本来は可視（marker=Inherited）→ visible:true で保存される。
        app.world_mut().spawn((
            WindowRoot,
            PanelKind::StrategyEditor,
            StrategyEditorId {
                region_key: "region_001".to_string(),
            },
            Transform::from_xyz(0.0, 0.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(500.0, 400.0)),
                ..default()
            },
            Visibility::Hidden, // mode system による一時的な強制 Hidden
            StrategyEditorModeHidden(Visibility::Inherited),
        ));
        // (b) layout が権威的に隠した editor（marker=Hidden）→ visible:false のまま。
        app.world_mut().spawn((
            WindowRoot,
            PanelKind::StrategyEditor,
            StrategyEditorId {
                region_key: "region_002".to_string(),
            },
            Transform::from_xyz(0.0, 0.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(500.0, 400.0)),
                ..default()
            },
            Visibility::Hidden,
            StrategyEditorModeHidden(Visibility::Hidden),
        ));

        let mut state: SystemState<(
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Transform,
                    &Sprite,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    Without<LayoutExcluded>,
                    Without<ScreenWindowRoot>,
                ),
            >,
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Node,
                    &GlobalZIndex,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    With<ScreenWindowRoot>,
                    Without<LayoutExcluded>,
                ),
            >,
            Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
            Res<StrategyBuffer>,
        )> = SystemState::new(app.world_mut());

        let (panels, screen_panels, camera, buffer) = state.get(app.world());
        let layout = build_layout(&panels, &screen_panels, &camera, &*buffer, None);

        let windows = layout.windows.expect("windows must be Some");
        let by_region = |key: &str| {
            windows
                .iter()
                .find(|w| w.region_key.as_deref() == Some(key))
                .unwrap_or_else(|| panic!("region {key} missing"))
        };
        assert!(
            by_region("region_001").visible,
            "forced-Hidden でもマーカーが Inherited なら visible:true で保存される"
        );
        assert!(
            !by_region("region_002").visible,
            "マーカーが Hidden（layout 権威で隠す意図）なら visible:false のまま"
        );
    }

    #[test]
    fn apply_layout_does_not_despawn_chart_when_layout_lacks_chart() {
        use crate::ui::components::ChartInstrument;
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let chart = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::Chart,
                Transform::from_xyz(10.0, 20.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(400.0, 300.0)),
                    ..default()
                },
                Visibility::Visible,
                ChartInstrument {
                    instrument_id: "7203.TSE".to_string(),
                },
                LayoutExcluded,
            ))
            .id();

        let tmp = std::env::temp_dir().join(format!(
            "ttwr_test_apply_no_chart_{}.json",
            std::process::id()
        ));
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": [{
                "kind": "Orders",
                "position": [0.0, 0.0],
                "size": [200.0, 150.0],
                "z": 1.0,
                "visible": true,
                "region_key": null
            }]
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.update();

        let _ = std::fs::remove_file(&tmp);

        assert!(
            app.world().get_entity(chart).is_ok(),
            "ChartInstrument 付き root は layout に含まれなくても despawn されない"
        );
    }

    /// issue #31 M1 回帰: Manual 中に layout load で Strategy Editor の可視性意図が変わったら、
    /// 退避マーカーが最新意図へ更新され、Manual 退出時に古い退避値で巻き戻さない。
    /// （mode system は live Visibility を Hidden に固定するので、apply_layout は本来の可視性を
    /// マーカーへ書く必要がある。これが無いとマーカーが陳腐化して退出時に意図と食い違う。）
    #[test]
    fn manual_layout_load_updates_mode_hidden_marker_intent() {
        use crate::trading::{ExecutionMode, ExecutionModeRes};
        use crate::ui::strategy_editor::apply_strategy_editor_mode_visibility_system;
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());
        app.init_resource::<ScenarioReadTarget>();
        app.init_resource::<ExecutionModeRes>(); // 既定 Replay

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // 既存の Strategy Editor 窓（region_001、最初は可視）。
        let editor = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::StrategyEditor,
                StrategyEditorId {
                    region_key: "region_001".to_string(),
                },
                Transform::from_xyz(0.0, 0.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(500.0, 400.0)),
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();

        // apply_layout → mode system の順に走らせる（mode が毎フレーム Hidden を維持する）。
        app.add_systems(
            Update,
            (
                apply_layout_system,
                apply_strategy_editor_mode_visibility_system,
            )
                .chain(),
        );

        // ── Manual 突入 → marker(Inherited) を退避し Hidden 化 ──
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
        app.update();
        assert_eq!(
            app.world().get::<StrategyEditorModeHidden>(editor).unwrap().0,
            Visibility::Inherited,
            "Manual 突入時のマーカーは突入前の可視性（Inherited）"
        );

        // ── Manual 中に layout load: region_001 を visible:false（隠す意図）に変更 ──
        let tmp =
            std::env::temp_dir().join(format!("ttwr_m1_marker_{}.json", std::process::id()));
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": [{
                "kind": "StrategyEditor",
                "position": [0.0, 0.0],
                "size": [500.0, 400.0],
                "z": 1.0,
                "visible": false,
                "region_key": "region_001"
            }]
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();
        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.update();
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(
            app.world().get::<StrategyEditorModeHidden>(editor).unwrap().0,
            Visibility::Hidden,
            "Manual 中の layout load でマーカーが最新意図（Hidden）へ更新される（陳腐化しない）"
        );
        assert_eq!(
            *app.world().get::<Visibility>(editor).unwrap(),
            Visibility::Hidden,
            "Manual 中は live Visibility は Hidden のまま"
        );

        // ── Manual 退出 → 最新意図 Hidden に復元（古い Inherited で巻き戻さない）──
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(editor).unwrap(),
            Visibility::Hidden,
            "layout が Hidden を意図したので Manual 退出後も Hidden（M1 回帰）"
        );
        assert!(
            app.world().get::<StrategyEditorModeHidden>(editor).is_none(),
            "Manual を抜けたらマーカーは除去される"
        );
    }

    /// issue #31 順序回帰: Manual 中に layout が `visible:false` で**新規 spawn** した
    /// Strategy Editor も、apply 系 → mode system の順序で正しいマーカーを捕捉し、
    /// Manual 退出時に layout 意図（Hidden）どおり Hidden のままになる。
    /// （production schedule の `apply_strategy_editor_mode_visibility_system.after(apply_*)` が
    /// 保証する不変条件。順序が逆だと spawn 既定値 Inherited を捕捉してマーカーが陳腐化する。）
    #[test]
    fn manual_late_spawned_hidden_editor_restores_hidden_on_exit() {
        use crate::trading::{ExecutionMode, ExecutionModeRes};
        use crate::ui::strategy_editor::apply_strategy_editor_mode_visibility_system;
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<PanelSpawnRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingStrategyFragments::default());
        app.init_resource::<ExecutionModeRes>();
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;

        // production と同じ順序: mode は apply_pending の後。
        app.add_systems(
            Update,
            (
                apply_pending_layout_system,
                apply_strategy_editor_mode_visibility_system.after(apply_pending_layout_system),
            ),
        );

        // 「新規 spawn 済みだが mode 未マーク」の StrategyEditor 窓（region_003, 既定 Inherited）。
        let late = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::StrategyEditor,
                StrategyEditorId {
                    region_key: "region_003".to_string(),
                },
                Transform::from_xyz(0.0, 0.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(500.0, 400.0)),
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();

        // apply_pending が処理する pending エントリ: region_003 を visible:false で意図。
        let mut pending = PendingLayoutApply::default();
        pending.windows.push(WindowLayout {
            kind: PanelKind::StrategyEditor,
            visible: false,
            position: [0.0, 0.0],
            size: [500.0, 400.0],
            z: 1.0,
            region_key: Some("region_003".to_string()),
        });
        app.insert_resource(pending);

        // 1 フレーム: apply_pending が intended(Hidden) を *vis に確定 → mode が Hidden を捕捉。
        app.update();
        assert_eq!(
            app.world().get::<StrategyEditorModeHidden>(late).unwrap().0,
            Visibility::Hidden,
            "Manual 中に処理された hidden 意図の新規窓はマーカーが Hidden を捕捉する"
        );

        // Manual 退出 → Hidden に復元（陳腐化した Inherited で巻き戻さない）。
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(late).unwrap(),
            Visibility::Hidden,
            "新規 spawn 窓も layout 意図(Hidden)どおり Manual 退出後に Hidden のまま"
        );
    }

    /// issue #31 順序回帰: save 系は mode system の前に走るので、Manual 突入「フレーム」で
    /// save が走っても、mode が live を Hidden に強制する前の値（＝本来の意図 = visible）を
    /// 読む。よって退避マーカーが deferred Commands でまだ反映されていなくても、
    /// forced-Hidden を `visible:false` として永続 layout に焼き込まない。
    #[test]
    fn save_before_mode_does_not_bake_forced_hidden_on_manual_entry_frame() {
        use crate::trading::{ExecutionMode, ExecutionModeRes};
        use crate::ui::strategy_editor::apply_strategy_editor_mode_visibility_system;
        use bevy::prelude::*;

        #[derive(Resource, Default)]
        struct CapturedVisible(Option<bool>);

        // "save" を模した system: build_layout を呼び StrategyEditor の visible を捕捉する。
        // production の save 系と同様に mode system の前に走らせる。
        #[allow(clippy::type_complexity)]
        fn capture_sys(
            panels: WorldPanelQuery<'_, '_>,
            screen_panels: ScreenPanelQuery<'_, '_>,
            camera: Query<
                (&Transform, &Projection),
                (With<Camera2d>, Without<WindowRoot>),
            >,
            buffer: Res<StrategyBuffer>,
            mut out: ResMut<CapturedVisible>,
        ) {
            let layout = build_layout(&panels, &screen_panels, &camera, &*buffer, None);
            out.0 = layout
                .windows
                .unwrap()
                .iter()
                .find(|w| w.kind == PanelKind::StrategyEditor)
                .map(|w| w.visible);
        }

        let mut app = App::new();
        app.insert_resource(StrategyBuffer::default());
        app.init_resource::<ExecutionModeRes>();
        app.init_resource::<CapturedVisible>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));
        let editor = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::StrategyEditor,
                StrategyEditorId {
                    region_key: "region_001".to_string(),
                },
                Transform::from_xyz(0.0, 0.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(500.0, 400.0)),
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();

        app.add_systems(
            Update,
            (
                capture_sys.before(apply_strategy_editor_mode_visibility_system),
                apply_strategy_editor_mode_visibility_system,
            ),
        );

        // Manual 突入フレームで capture(save 相当) が走る。
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
        app.update();

        assert_eq!(
            app.world().resource::<CapturedVisible>().0,
            Some(true),
            "Manual 突入フレームでも save は mode 強制前に読むので visible:true を保存する（forced-Hidden を焼き込まない）"
        );
        // 一方 editor 自体は mode により Hidden になっている。
        assert_eq!(
            *app.world().get::<Visibility>(editor).unwrap(),
            Visibility::Hidden,
            "save 後に走る mode system が live を Hidden に強制している"
        );
    }

    /// Bug repro (RED): scenario-only JSON (windows=None, strategy_path=None) を
    /// `UserJsonOpen` で開き、sibling `.py` が存在しないとき、
    /// `apply_layout_system` は `ScenarioReadTarget = Some(event.path)` をセットして
    /// scenario_parser_system に JSON を再 parse させるべき。
    /// 現状は素通りして ScenarioReadTarget が更新されず、UI に scenario が反映されない。
    #[test]
    fn scenario_only_json_without_sibling_py_updates_scenario_read_target() {
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());
        app.init_resource::<ScenarioReadTarget>();

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // scenario-only JSON: schema_version / windows / strategy_path いずれも無し。
        // sibling `.py` は作らない（バグ条件）。
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("s7_no_sibling.json");
        let body = r#"{"scenario":{"instrument":"7203.TSE","start":"2025-01-06","end":"2025-03-31","granularity":"Daily","initial_cash":1000000}}"#;
        std::fs::write(&json_path, body).unwrap();
        assert!(
            !json_path.with_extension("py").exists(),
            "precondition: sibling .py は存在しないこと"
        );

        app.world_mut().send_event(LayoutLoadRequested {
            path: json_path.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.add_systems(Update, apply_layout_system);
        app.update();

        let target = app.world().resource::<ScenarioReadTarget>();
        assert_eq!(
            target.0.as_ref(),
            Some(&json_path),
            "scenario-only JSON without sibling .py must set ScenarioReadTarget so \
             scenario_parser_system re-parses the JSON; got {:?}",
            target.0
        );
    }

    #[test]
    fn picker_window_is_excluded_from_layout_save() {
        use crate::ui::instrument_picker::InstrumentPickerWindow;
        use bevy::ecs::system::SystemState;
        use bevy::prelude::*;

        let mut app = App::new();
        app.insert_resource(StrategyBuffer::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Orders,
            Transform::from_xyz(0.0, 0.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(200.0, 150.0)),
                ..default()
            },
            Visibility::Visible,
        ));

        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Orders,
            Transform::from_xyz(100.0, 100.0, 2.0),
            Sprite {
                custom_size: Some(Vec2::new(360.0, 480.0)),
                ..default()
            },
            Visibility::Visible,
            InstrumentPickerWindow,
            LayoutExcluded,
        ));

        let mut state: SystemState<(
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Transform,
                    &Sprite,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    Without<LayoutExcluded>,
                    Without<ScreenWindowRoot>,
                ),
            >,
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Node,
                    &GlobalZIndex,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    With<ScreenWindowRoot>,
                    Without<LayoutExcluded>,
                ),
            >,
            Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
            Res<StrategyBuffer>,
        )> = SystemState::new(app.world_mut());

        let (panels, screen_panels, camera, buffer) = state.get(app.world());
        let layout = build_layout(&panels, &screen_panels, &camera, &*buffer, None);

        let windows = layout.windows.expect("windows must be Some");
        assert_eq!(
            windows.len(),
            1,
            "LayoutExcluded 付き picker window は除外され、Orders 1 件のみ残る"
        );
        assert_eq!(windows[0].kind, PanelKind::Orders);
    }

    #[test]
    fn live_order_window_is_excluded_from_layout_save() {
        use bevy::ecs::system::SystemState;
        use bevy::prelude::*;

        let mut app = App::new();
        app.insert_resource(StrategyBuffer::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // LIVE Order window: no LayoutExcluded. scenario 所有 (restore_driver==ScenarioInstruments)
        // なので save 経路で弾かれるべき (issue #25)。
        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Order,
            Transform::from_xyz(10.0, 20.0, 3.0),
            Sprite {
                custom_size: Some(Vec2::new(300.0, 200.0)),
                ..default()
            },
            Visibility::Visible,
        ));

        // 通常 panel: 残るべき。
        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Orders,
            Transform::from_xyz(0.0, 0.0, 1.0),
            Sprite {
                custom_size: Some(Vec2::new(200.0, 150.0)),
                ..default()
            },
            Visibility::Visible,
        ));

        let mut state: SystemState<(
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Transform,
                    &Sprite,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    Without<LayoutExcluded>,
                    Without<ScreenWindowRoot>,
                ),
            >,
            Query<
                (
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&StrategyEditorModeHidden>,
                    &Node,
                    &GlobalZIndex,
                    &Visibility,
                ),
                (
                    With<WindowRoot>,
                    With<ScreenWindowRoot>,
                    Without<LayoutExcluded>,
                ),
            >,
            Query<(&Transform, &Projection), (With<Camera2d>, Without<WindowRoot>)>,
            Res<StrategyBuffer>,
        )> = SystemState::new(app.world_mut());

        let (panels, screen_panels, camera, buffer) = state.get(app.world());
        let layout = build_layout(&panels, &screen_panels, &camera, &*buffer, None);

        let windows = layout.windows.expect("windows must be Some");
        assert!(
            !windows.iter().any(|w| w.kind == PanelKind::Order),
            "PanelKind::Order は scenario 所有なので save の windows[] に混入してはならない (issue #25)"
        );
        assert_eq!(windows.len(), 1, "Orders 1 件だけが残る");
        assert_eq!(windows[0].kind, PanelKind::Orders);
    }

    #[test]
    fn layout_restore_does_not_spawn_picker_window() {
        use crate::ui::instrument_picker::InstrumentPickerWindow;
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let picker = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::Orders,
                Transform::from_xyz(100.0, 100.0, 2.0),
                Sprite {
                    custom_size: Some(Vec2::new(360.0, 480.0)),
                    ..default()
                },
                Visibility::Visible,
                InstrumentPickerWindow,
                LayoutExcluded,
            ))
            .id();

        let tmp = std::env::temp_dir().join(format!(
            "ttwr_test_apply_no_picker_{}.json",
            std::process::id()
        ));
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": [{
                "kind": "Orders",
                "position": [0.0, 0.0],
                "size": [200.0, 150.0],
                "z": 1.0,
                "visible": true,
                "region_key": null
            }]
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.update();

        let _ = std::fs::remove_file(&tmp);

        assert!(
            app.world().get_entity(picker).is_ok(),
            "InstrumentPickerWindow root は layout に含まれなくても despawn されない"
        );
    }

    #[test]
    fn layout_restore_does_not_spawn_order_window() {
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let tmp = std::env::temp_dir().join(format!(
            "ttwr_test_apply_no_order_{}.json",
            std::process::id()
        ));
        // JSON に Order window が混入していても restore-skip により再 spawn されない。
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": [{
                "kind": "Order",
                "position": [0.0, 0.0],
                "size": [320.0, 360.0],
                "z": 1.0,
                "visible": true,
                "region_key": null
            }]
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.update();

        let _ = std::fs::remove_file(&tmp);

        let mut spawn_events = app
            .world_mut()
            .resource_mut::<Events<PanelSpawnRequested>>();
        let order_spawns: Vec<PanelKind> = spawn_events
            .update_drain()
            .map(|ev| ev.kind)
            .filter(|k| *k == PanelKind::Order)
            .collect();
        assert!(
            order_spawns.is_empty(),
            "Order は restore_driver==ScenarioInstruments のため layout restore で再 spawn されない。got = {:?}",
            order_spawns
        );
    }

    #[test]
    fn layout_restore_does_not_force_startup_visibility() {
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // Startup root は ExecutionMode が可視性を所有する。restore 前は Inherited。
        let startup = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::Startup,
                Transform::from_xyz(0.0, 0.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(100.0, 100.0)),
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();

        let tmp = std::env::temp_dir().join(format!(
            "ttwr_test_startup_vis_{}.json",
            std::process::id()
        ));
        // visible:false を含む layout。pos/z は復元、visible は無視されるべき。
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": [{
                "kind": "Startup",
                "position": [42.0, 24.0],
                "size": [100.0, 100.0],
                "z": 7.0,
                "visible": false,
                "region_key": null
            }]
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.update();

        let _ = std::fs::remove_file(&tmp);

        let vis = app.world().get::<Visibility>(startup).unwrap();
        assert!(
            !matches!(vis, Visibility::Hidden),
            "restore は Startup の Visibility を Hidden に強制してはいけない \
             (可視性は ExecutionMode が所有する)"
        );
        let tf = app.world().get::<Transform>(startup).unwrap();
        assert_eq!(tf.translation.x, 42.0, "Startup の位置 x は復元される");
        assert_eq!(tf.translation.y, 24.0, "Startup の位置 y は復元される");
        assert_eq!(tf.translation.z, 7.0, "Startup の z は復元される");
    }

    #[test]
    fn apply_layout_keeps_startup_when_absent_from_windows() {
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let startup = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::Startup,
                Transform::from_xyz(0.0, 0.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(260.0, 200.0)),
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();

        let tmp = std::env::temp_dir().join(format!(
            "ttwr_test_startup_absent_{}.json",
            std::process::id()
        ));
        // Startup を含まない windows リスト（pre-#14 sidecar 相当）。
        // 「list に無い → despawn」対象から Startup は除外されるべき。
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": []
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.update();

        let _ = std::fs::remove_file(&tmp);

        assert!(
            app.world().get_entity(startup).is_ok(),
            "Startup は windows リストに無くても despawn されない \
             (起動時 spawn・ExecutionMode 可視性が所有。再 spawn 経路はフィールド無し)"
        );
    }

    #[test]
    fn apply_layout_does_not_resize_startup() {
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // 現行の窓サイズ 320×200 で spawn。
        let startup = app
            .world_mut()
            .spawn((
                WindowRoot,
                PanelKind::Startup,
                Transform::from_xyz(0.0, 0.0, 1.0),
                Sprite {
                    custom_size: Some(Vec2::new(320.0, 200.0)),
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();

        let tmp = std::env::temp_dir().join(format!(
            "ttwr_test_startup_size_{}.json",
            std::process::id()
        ));
        // 古い 260 幅を含む layout。pos/z は復元するが size は当てない。
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": null,
            "windows": [{
                "kind": "Startup",
                "position": [10.0, 20.0],
                "size": [260.0, 200.0],
                "z": 5.0,
                "visible": true,
                "region_key": null
            }]
        });
        std::fs::write(&tmp, serde_json::to_string(&layout_json).unwrap()).unwrap();

        app.world_mut().send_event(LayoutLoadRequested {
            path: tmp.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.update();

        let _ = std::fs::remove_file(&tmp);

        let sprite = app.world().get::<Sprite>(startup).unwrap();
        assert_eq!(
            sprite.custom_size,
            Some(Vec2::new(320.0, 200.0)),
            "Startup の size は layout から復元しない（古い 260 幅に巻き戻らない）"
        );
        let tf = app.world().get::<Transform>(startup).unwrap();
        assert_eq!(tf.translation.x, 10.0, "Startup の位置 x は復元される");
        assert_eq!(tf.translation.y, 20.0, "Startup の位置 y は復元される");
        assert_eq!(tf.translation.z, 5.0, "Startup の z は復元される");
    }

    #[test]
    fn save_layout_writes_registry_to_original_sidecar() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let cache_json_path = dir.path().join("cache.json");
        let initial = r#"{"scenario":{"schema_version":2,"instruments":["1301.TSE","7203.TSE"],"start":"2025-01-06","end":"2025-01-10","granularity":"Minute","initial_cash":1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial).unwrap();
        std::fs::write(&cache_json_path, initial).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["1301.TSE".to_string()];
        reg.editable = true;
        app.insert_resource(reg);
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<ScenarioReadTarget>();

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, handle_save_layout_system);
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        let body = std::fs::read_to_string(&json_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let got: Vec<String> = v["scenario"]["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert_eq!(got, vec!["1301.TSE".to_string()]);
    }

    /// E2E-4: picker entity (`InstrumentPickerWindow` + `LayoutExcluded`) は
    /// save 時の JSON `windows[]` に混入せず、load 時にも `PanelSpawnRequested` を
    /// 発火させない。Orders panel (LayoutExcluded なし) のみが round-trip する。
    #[test]
    fn picker_excluded_from_layout_roundtrip() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use crate::ui::instrument_picker::InstrumentPickerWindow;
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let cache_json_path = dir.path().join("cache.json");
        let initial = r#"{"scenario":{"schema_version":2,"instruments":["1301.TSE"],"start":"2025-01-06","end":"2025-01-10","granularity":"Minute","initial_cash":1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial).unwrap();
        std::fs::write(&cache_json_path, initial).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["1301.TSE".to_string()];
        reg.editable = true;
        app.insert_resource(reg);
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<ScenarioReadTarget>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(PendingStrategyFragments::default());

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        // Orders panel: save 経路の query (With<WindowRoot>, Without<LayoutExcluded>) に拾われる。
        app.world_mut().spawn((
            WindowRoot,
            PanelKind::Orders,
            Transform::from_xyz(50.0, 60.0, 1.5),
            Sprite {
                custom_size: Some(Vec2::new(200.0, 150.0)),
                ..default()
            },
            Visibility::Visible,
        ));

        // Picker entity: LayoutExcluded を持つので save から除外されるはず。
        app.world_mut().spawn((
            WindowRoot,
            Transform::from_xyz(100.0, 100.0, 2.0),
            Sprite {
                custom_size: Some(Vec2::new(360.0, 480.0)),
                ..default()
            },
            Visibility::Visible,
            InstrumentPickerWindow,
            LayoutExcluded,
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, (handle_save_layout_system, apply_layout_system));

        // --- Save ---
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        let body = std::fs::read_to_string(&json_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let windows = v["windows"].as_array().expect("windows must be array");
        assert_eq!(
            windows.len(),
            1,
            "picker は LayoutExcluded で除外され、Orders 1 件だけが windows[] に残る"
        );
        assert_eq!(
            windows[0]["kind"].as_str(),
            Some("Orders"),
            "残った 1 件は Orders kind であるべき"
        );

        // --- Load ---
        // Save 直後の Orders entity は既に world に存在するので、
        // apply_layout_system は found Some 経路に入り PanelSpawnRequested は発火しない想定。
        // ただし「picker が JSON に混ざっていた」場合は found None 経路で picker kind の
        // PanelSpawnRequested が発火するため、ここで検知できる。
        app.world_mut().send_event(LayoutLoadRequested {
            path: json_path.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.update();

        let mut spawn_events = app
            .world_mut()
            .resource_mut::<Events<PanelSpawnRequested>>();
        let kinds: Vec<PanelKind> = spawn_events.update_drain().map(|ev| ev.kind).collect();
        assert!(
            kinds.is_empty(),
            "Orders は既存 entity が match するため再 spawn されず、\
             picker は JSON に混入していないため発火しない。got = {:?}",
            kinds
        );
    }

    #[test]
    fn save_layout_skip_when_scenario_required_fields_missing() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let cache_json_path = dir.path().join("cache.json");
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, "{}").unwrap();
        std::fs::write(&cache_json_path, "{}").unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["1301.TSE".to_string()];
        reg.editable = true;
        app.insert_resource(reg);
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<ScenarioReadTarget>();

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, handle_save_layout_system);
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        let body = std::fs::read_to_string(&json_path).unwrap();
        assert_eq!(body, "{}", "scenario 欠落時は元 .json を上書きしない");

        let buffer = app.world().resource::<StrategyBuffer>();
        assert_eq!(buffer.original_path.as_deref(), Some(py_path.as_path()));
    }

    /// A2: cache_sidecar = None でも元 sidecar (buffer.original_path.with_extension("json"))
    /// に registry 内容で writeback できる。
    #[test]
    fn save_layout_continues_when_cache_sidecar_path_none() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let initial = r#"{"scenario":{"schema_version":2,"instruments":["1301.TSE","7203.TSE"],"start":"2025-01-06","end":"2025-01-10","granularity":"Minute","initial_cash":1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: None,
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["6758.TSE".to_string()];
        reg.editable = true;
        app.insert_resource(reg);
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<ScenarioReadTarget>();

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, handle_save_layout_system);
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        // cache_sidecar 関連 path が None のまま
        let paths_res = app.world().resource::<ScenarioWritebackPaths>();
        assert!(paths_res.cache_sidecar.is_none());

        // 元 sidecar に registry が反映されている
        let body = std::fs::read_to_string(&json_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let got: Vec<String> = v["scenario"]["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert_eq!(got, vec!["6758.TSE".to_string()]);
    }

    /// A4: cache JSON が壊れているとき、ScenarioMetadata から最小 v2 を再構築する経路で
    /// 元 sidecar 側に registry 値が届く。
    #[test]
    fn save_layout_fallback_to_original_sidecar_when_cache_corrupt() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let cache_json_path = dir.path().join("cache.json");
        let initial = r#"{"scenario":{"schema_version":2,"instruments":["1301.TSE","7203.TSE"],"start":"2025-01-06","end":"2025-01-10","granularity":"Minute","initial_cash":1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial).unwrap();
        std::fs::write(&cache_json_path, "{ not json").unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["9984.TSE".to_string()];
        reg.editable = true;
        app.insert_resource(reg);
        // ScenarioMetadata を埋めておく: cache 不正で preserve が None になっても
        // build_layout_for_explicit_save が ScenarioMetadata から最小 v2 を再構築する。
        let mut meta = ScenarioMetadata::default();
        meta.schema_version = Some(2);
        meta.start = Some("2025-01-06".to_string());
        meta.end = Some("2025-01-10".to_string());
        meta.granularity = Some("Minute".to_string());
        meta.initial_cash = Some(1000000);
        app.insert_resource(meta);
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<ScenarioReadTarget>();

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, handle_save_layout_system);
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        // 元 sidecar 側に registry 値が反映されている
        // (NOTE: cache 自体が修復されるかは実装依存なので assert しない)
        let body = std::fs::read_to_string(&json_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let got: Vec<String> = v["scenario"]["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert_eq!(got, vec!["9984.TSE".to_string()]);
    }

    /// A5: registry.editable = false (instruments_ref ロック) のとき、
    /// 元 sidecar の `instruments_ref` 形状を破壊しない。
    #[test]
    fn save_layout_preserves_instruments_ref_shape() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        // scenario.instruments_ref を持つ最小 sidecar (instruments キーは持たない)
        let initial = r#"{"scenario":{"schema_version":3,"instruments_ref":"universe/foo.json","start":"2025-01-06","end":"2025-01-10","granularity":"Daily","initial_cash":1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        // cache_sidecar=None にして fallback (元 sidecar) を preserve 源にする
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: None,
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["1301.TSE".to_string(), "7203.TSE".to_string()];
        reg.editable = false; // instruments_ref ロック
        app.insert_resource(reg);
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<ScenarioReadTarget>();

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, handle_save_layout_system);
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        let body = std::fs::read_to_string(&json_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let scenario = &v["scenario"];
        assert!(
            scenario.get("instruments_ref").is_some(),
            "instruments_ref must be preserved"
        );
        assert_eq!(scenario["instruments_ref"], "universe/foo.json");
        assert!(
            scenario.get("instruments").is_none(),
            "registry must not flatten into instruments key when editable=false"
        );
    }

    /// A6: handle_save_layout_system 経路で
    ///   - 元 strat.json の instruments が registry の最新値で上書きされる (writeback 効いている)
    ///   - scenario 内の original-only field (例: marker_original) は cache restore 仕様により落ちるのが仕様
    ///   - cache_sidecar 側の preserve 対象フィールド (marker_cache) は writeback 後も保持される
    ///   - 元 .py が byte 不変
    ///   - cache_sidecar JSON は KC4-a の pre-flush 仕様により registry 最新値で上書きされる
    ///     (A2/A4/A5 系と一貫)
    #[test]
    fn save_layout_writes_registry_and_preserves_markers_and_py_bytes() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioMetadata, ScenarioWritebackPaths, StrategyBuffer,
        };
        use bevy::prelude::*;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let cache_json_path = dir.path().join("cache.json");

        // 元 strat.json: 古い instruments + marker_original を保持
        let original_json = r#"{"scenario":{"schema_version":2,"instruments":["OLD1.TSE","OLD2.TSE"],"start":"2025-01-06","end":"2025-01-10","granularity":"Minute","initial_cash":1000000,"marker_original":"keep-me"}}"#;
        // cache_sidecar: 別の古い instruments + cache 側固有マーカーを持つファイル
        let cache_json = r#"{"scenario":{"schema_version":2,"instruments":["CACHE_OLD.TSE"],"start":"2025-01-06","end":"2025-01-10","granularity":"Minute","initial_cash":1000000,"marker_cache":"from_cache"}}"#;
        let py_bytes: &[u8] = b"# original python body\nprint('hi')\n";
        std::fs::write(&py_path, py_bytes).unwrap();
        std::fs::write(&json_path, original_json).unwrap();
        std::fs::write(&cache_json_path, cache_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        let mut reg = InstrumentRegistry::default();
        reg.ids = vec!["NEW1.TSE".to_string(), "NEW2.TSE".to_string()];
        reg.editable = true;
        app.insert_resource(reg);
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<StrategyAutoSaveState>();
        app.insert_resource(ScenarioReadTarget(Some(cache_json_path.clone())));

        app.add_event::<LayoutSaveRequested>();
        app.add_event::<LayoutSaveAsRequested>();
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        app.init_resource::<PendingFileDialog>();
        app.add_systems(Update, handle_save_layout_system);
        app.world_mut().send_event(LayoutSaveRequested);
        app.update();

        // ① 元 strat.json の instruments が registry の最新値で上書きされている
        let body = std::fs::read_to_string(&json_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let got: Vec<String> = v["scenario"]["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            got,
            vec!["NEW1.TSE".to_string(), "NEW2.TSE".to_string()],
            "registry must be written back to original sidecar"
        );

        // ② scenario 内の original-only field は cache restore 仕様により落ちる (regression 固定)
        assert!(
            v["scenario"]
                .get("marker_original")
                .map_or(true, |x| x.is_null()),
            "marker_original from source strat.json must NOT survive writeback (cache is source of truth)"
        );
        // ②' cache_sidecar 側マーカーは preserve される
        assert_eq!(
            v["scenario"]["marker_cache"],
            serde_json::json!("from_cache"),
            "marker from cache_sidecar must be preserved through writeback"
        );

        // ③ 元 .py が byte 不変
        let py_after = std::fs::read(&py_path).unwrap();
        assert_eq!(
            py_after.as_slice(),
            py_bytes,
            "original .py must be byte-identical"
        );

        // ④ cache_sidecar は KC4-a 仕様により registry 最新値で上書きされる
        let cache_body = std::fs::read_to_string(&cache_json_path).unwrap();
        let cv: serde_json::Value = serde_json::from_str(&cache_body).unwrap();
        let cache_got: Vec<String> = cv["scenario"]["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            cache_got,
            vec!["NEW1.TSE".to_string(), "NEW2.TSE".to_string()],
            "cache_sidecar must be flushed with registry latest values (KC4-a)"
        );
    }

    /// UserJsonOpen は pending_fragments.loaded_for_path == strategy_path であっても
    /// JSON の strategy_path を必ず再読込する（StrategyFileLoadRequested を発火する）。
    #[test]
    fn test_user_json_open_reloads_strategy_path_even_if_already_loaded() {
        use crate::ui::components::{PendingStrategyFragments, RegionKeyAllocator, StrategyBuffer};
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(StrategyBuffer::default());
        app.insert_resource(PendingStrategyFragments::default());
        app.insert_resource(RegionKeyAllocator::default());
        app.init_resource::<Time>();

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("foo.py");
        let json_path = dir.path().join("foo.json");
        std::fs::write(&py_path, "# --- region: region_001 ---\npass\n").unwrap();
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": py_path.to_string_lossy(),
            "windows": [],
        });
        std::fs::write(&json_path, serde_json::to_string(&layout_json).unwrap()).unwrap();

        // 既に foo.py を loaded 済みの状態を作る（UserOpen 由来）。
        {
            let mut pf = app.world_mut().resource_mut::<PendingStrategyFragments>();
            pf.loaded_for_path = Some(py_path.clone());
        }

        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.world_mut().send_event(LayoutLoadRequested {
            path: json_path.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.update();
        app.update();

        let mut load_events = app
            .world_mut()
            .resource_mut::<Events<StrategyFileLoadRequested>>();
        let loads: Vec<StrategyFileLoadRequested> = load_events.update_drain().collect();
        assert_eq!(
            loads.len(),
            1,
            "UserJsonOpen は pending_fragments を無視して strategy_path を必ず reload するべき。\
             got events = {}",
            loads.len()
        );
        assert_eq!(
            loads[0].path, py_path,
            "reload された path は JSON の strategy_path であるべき"
        );
    }

    /// cache restore 後に残った pending_fragments.by_region_key を、
    /// 後続の UserJsonOpen が deferred path に入るときに必ず破棄する。
    #[test]
    fn user_json_open_clears_stale_pending_fragments() {
        use crate::ui::components::{PendingStrategyFragments, RegionKeyAllocator, StrategyBuffer};
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(StrategyBuffer::default());
        app.insert_resource(PendingStrategyFragments::default());
        app.insert_resource(RegionKeyAllocator::default());
        app.init_resource::<Time>();

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("foo.py");
        let json_path = dir.path().join("foo.json");
        std::fs::write(&py_path, "# --- region: region_001 ---\npass\n").unwrap();
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": py_path.to_string_lossy(),
            "windows": [],
        });
        std::fs::write(&json_path, serde_json::to_string(&layout_json).unwrap()).unwrap();

        // cache restore で投入された stale fragments を仕込む。
        {
            let mut pf = app.world_mut().resource_mut::<PendingStrategyFragments>();
            pf.by_region_key
                .insert("region_999".to_string(), "stale body".to_string());
            pf.loaded_for_path = Some(PathBuf::from("/some/other/path.py"));
        }

        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.world_mut().send_event(LayoutLoadRequested {
            path: json_path.clone(),
            mode: LayoutLoadMode::UserJsonOpen,
        });
        app.update();

        let pf = app.world().resource::<PendingStrategyFragments>();
        assert!(
            pf.by_region_key.is_empty(),
            "UserJsonOpen deferred path must clear stale pending_fragments.by_region_key, \
             got: {:?}",
            pf.by_region_key.keys().collect::<Vec<_>>()
        );
    }

    /// ApplySidecarForPy は `.py` UserOpen 後の sidecar 適用なので、
    /// sidecar の strategy_path が別ファイルを指していても読み替えない。
    #[test]
    fn apply_sidecar_for_py_ignores_mismatched_strategy_path() {
        use crate::ui::components::{PendingStrategyFragments, RegionKeyAllocator, StrategyBuffer};
        use bevy::prelude::*;

        let mut app = App::new();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<StrategyFileLoadRequested>();
        app.insert_resource(WindowManager::default());
        app.insert_resource(PendingLayoutApply::default());
        app.insert_resource(StrategyBuffer::default());
        app.insert_resource(PendingStrategyFragments::default());
        app.insert_resource(RegionKeyAllocator::default());
        app.init_resource::<Time>();

        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));

        let dir = tempfile::tempdir().unwrap();
        let user_py_path = dir.path().join("user_selected.py");
        let sidecar_py_path = dir.path().join("sidecar_points_elsewhere.py");
        let json_path = dir.path().join("user_selected.json");
        std::fs::write(
            &user_py_path,
            "# region region_001\nuser\n# endregion region_001\n",
        )
        .unwrap();
        std::fs::write(
            &sidecar_py_path,
            "# region region_001\nsidecar\n# endregion region_001\n",
        )
        .unwrap();
        let layout_json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "viewport": null,
            "strategy_path": sidecar_py_path.to_string_lossy(),
            "windows": [],
        });
        std::fs::write(&json_path, serde_json::to_string(&layout_json).unwrap()).unwrap();

        {
            let mut pf = app.world_mut().resource_mut::<PendingStrategyFragments>();
            pf.loaded_for_path = Some(user_py_path.clone());
            pf.by_region_key
                .insert("region_001".to_string(), "user".to_string());
        }

        app.init_resource::<ScenarioReadTarget>();
        app.add_systems(Update, apply_layout_system);
        app.world_mut().send_event(LayoutLoadRequested {
            path: json_path,
            mode: LayoutLoadMode::ApplySidecarForPy,
        });
        app.update();
        app.update();

        let mut load_events = app
            .world_mut()
            .resource_mut::<Events<StrategyFileLoadRequested>>();
        let loads: Vec<StrategyFileLoadRequested> = load_events.update_drain().collect();
        assert!(
            loads.is_empty(),
            "ApplySidecarForPy must not reload sidecar strategy_path when user-selected .py differs"
        );

        let pf = app.world().resource::<PendingStrategyFragments>();
        assert_eq!(pf.loaded_for_path.as_deref(), Some(user_py_path.as_path()));
        assert_eq!(
            pf.by_region_key.get("region_001").map(String::as_str),
            Some("user")
        );
    }
}
