use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Resource, Default)]
pub struct WindowManager {
    pub max_z: f32,
}

#[derive(Component)]
pub struct WindowRoot;

#[derive(Component)]
pub struct TitleBar;

#[derive(Component)]
pub struct PriceDisplay;

#[derive(Component)]
pub struct StatusIndicator;

#[derive(Component, Clone, Copy)]
pub enum TradeButton {
    Buy,
    Sell,
}

#[derive(Component)]
pub struct FooterRoot;

#[derive(Component)]
pub struct ReplayTimeLabel;

#[derive(Component)]
pub struct ReplayStateBadge;

#[derive(Component)]
pub struct GrpcStatusLabel;

#[derive(Component)]
pub struct PauseResumeLabel;

/// Marker for the footer PauseResume Button entity (NOT the Text child).
/// `PauseResumeLabel` marks the inner Text node for label swapping; this one
/// marks the Button entity itself so `footer_pause_resume_system` can query it.
#[derive(Component)]
pub struct PauseResumeButton;

#[derive(Component, Clone, Copy, Debug)]
pub enum TransportButton {
    JumpToStart,
    StepBack,
    PauseResume,
    StepForward,
    ForceStop,
}

/// Marks a speed-selector button in the footer. Holds the multiplier value (1, 2, 5, 10, 50).
#[derive(Component, Clone, Copy, Debug)]
pub struct SpeedButton(pub u32);

#[derive(Component)]
pub struct MenuBarRoot;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTopLevel {
    File,
    Edit,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    SaveLayout,
    SaveLayoutAs,
    LoadLayout,
    Undo,
    Redo,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuPopup(pub MenuTopLevel);

#[derive(Resource, Default)]
pub struct OpenMenu(pub Option<MenuTopLevel>);

#[derive(Resource, Default, Debug, Clone)]
pub struct StrategyBuffer {
    pub original_path: Option<PathBuf>,
    pub cache_path: Option<PathBuf>,
    /// `merge_and_flush_to_cache` が成功したときのみ書き込む読み取り専用ビュー。
    /// ステータスラベル表示とテストに使う。マージ済みソースのキャッシュは持たない。
    pub last_merged_source: Option<String>,
}

#[derive(Component)]
pub struct StrategyStatusLabel;

#[derive(Event, Debug, Clone)]
pub struct StrategyRunRequested {
    pub cache_path: std::path::PathBuf,
}

#[derive(Component)]
pub struct SidebarRoot;

/// Single text node that shows loading / error / empty / instrument list.
#[derive(Component)]
pub struct SidebarListLabel;

#[derive(Component, Debug, Clone)]
pub struct SidebarInstrumentRow {
    pub instrument_id: String,
}

#[derive(Component, Debug, Clone)]
pub struct SidebarInstrumentRemoveButton {
    pub instrument_id: String,
}

#[derive(Component, Debug)]
pub struct SidebarInstrumentsList;

#[derive(Component, Debug)]
pub struct SidebarInstrumentsWarning;

/// floating window の × ボタンに貼るマーカー。
/// Click observer がこの entity の祖先 WindowRoot を Visibility::Hidden にする。
#[derive(Component)]
pub struct CloseButton;

#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioMetadata {
    pub schema_version: Option<u32>,
    /// Normalized instrument list (handles both "instrument" str/list and "instruments" list)
    pub instruments: Vec<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub initial_cash: Option<i64>,
}

/// 6 種類すべての floating window を区別するための種別タグ。
/// サイドバーのボタン entity と、spawn された panel root entity の両方に貼る。
/// Sub-step 1.2 で「既に spawn 済みかどうか」を判定するのにも使う。
#[derive(
    Component, Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PanelKind {
    Chart,
    StrategyEditor,
    BuyingPower,
    RunResult,
    Positions,
    Orders,
}

impl PanelKind {
    /// サイドバーのボタンに表示する文字列。
    pub fn label(self) -> &'static str {
        match self {
            PanelKind::Chart => "Chart",
            PanelKind::StrategyEditor => "Strategy Editor",
            PanelKind::BuyingPower => "Buying Power",
            PanelKind::RunResult => "Run Result",
            PanelKind::Positions => "Positions",
            PanelKind::Orders => "Orders",
        }
    }
}

/// パネル spawn の発生源を区別するための種別。
/// `panel_spawn_dispatcher_system` が WindowSpawnEdit を push するかどうかの判定に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSpawnSource {
    /// サイドバーのボタンなど、ユーザー操作による spawn。
    User,
    /// レイアウト JSON ロード時の自動 spawn。
    LayoutLoad,
    /// Undo/Redo による spawn。
    UndoRedo,
}

/// パネルボタンが押されたとき発火するイベント。
/// `panel_spawn_dispatcher_system` が受け取り、未スポーンなら spawn する。
#[derive(Event, Debug, Clone)]
pub struct PanelSpawnRequested {
    pub kind: PanelKind,
    pub source: PanelSpawnSource,
    /// StrategyEditor spawn 時のみ Some。blank spawn は `StrategyEditorSpawnSpec::default()` を使う。
    pub strategy_spec: Option<StrategyEditorSpawnSpec>,
}

#[derive(Event, Debug, Clone)]
pub struct UndoMenuRequested;

#[derive(Event, Debug, Clone)]
pub struct RedoMenuRequested;

// ─── Strategy Editor multi-spawn 用型群 ───────────────────────────────────

/// 各 StrategyEditor window が持つ一意キー。root entity と editor child entity の
/// 両方に貼ることで、どちらからでも region_key で対応 entity を逆引きできる。
#[derive(Component, Debug, Clone)]
pub struct StrategyEditorId {
    pub region_key: String,
}

/// root window entity にのみ置くソース断片。editor child は持たない（単一オーナー）。
#[derive(Component, Debug, Clone, Default)]
pub struct StrategyFragment {
    pub source: String,
    pub dirty: bool,
}

/// region_key の連番を管理する Resource。
/// allocate() は常に新しいキーを返す。
/// bump_to_at_least() はレイアウト復元時に既存の最大番号に合わせる。
#[derive(Resource, Default)]
pub struct RegionKeyAllocator {
    pub next: u32,
}

impl RegionKeyAllocator {
    pub fn allocate(&mut self) -> String {
        self.next += 1;
        format!("region_{:03}", self.next)
    }

    /// レイアウト復元時に既存 region_key の番号を追い越さないよう上限を合わせる。
    pub fn bump_to_at_least(&mut self, n: u32) {
        self.next = self.next.max(n);
    }
}

/// ファイルロード後に各 region_key → source を保持する一時 Resource。
/// `handle_strategy_file_load_system` が詰め、`panel_spawn_dispatcher_system` が drain する。
/// drain 後のエントリは残らないので古いロードのゴミが残るリスクがない。
#[derive(Resource, Default, Debug)]
pub struct PendingStrategyFragments {
    /// region_key → source body（マーカー行・末尾 \n を除く）
    pub by_region_key: HashMap<String, String>,
    /// このバッチを解析した .py パス。drain 時に不一致なら warn して skip する。
    pub loaded_for_path: Option<PathBuf>,
}

/// ユーザー操作またはレイアウト復元でストラテジーファイルをロードするイベント。
/// 旧 `OpenStrategyRequested` を置き換え、mode によって read→split→spawn の
/// 判断木を `handle_strategy_file_load_system` 一か所に集約する。
#[derive(Event, Debug, Clone)]
pub struct StrategyFileLoadRequested {
    pub path: PathBuf,
    pub mode: StrategyLoadMode,
}

/// ファイルロードの発生元を区別する。
/// handler 内部の分岐条件として使い、サイドカー適用・サプレスなどを切り替える。
#[derive(Debug, Clone, Copy)]
pub enum StrategyLoadMode {
    /// ユーザーが Load Layout から .py を選択したときのロードモード。サイドカーが存在すれば適用、なければ全置換。
    UserOpen,
    /// レイアウト JSON の strategy_path フィールド由来。スポーン配置はレイアウトが決定済み。
    LayoutRestore,
}

/// `PanelSpawnRequested` に同梱して `panel_spawn_dispatcher_system` に渡す引数。
/// StrategyEditor 以外の PanelKind には使わない（`strategy_spec: None` のまま）。
#[derive(Debug, Clone)]
pub struct StrategyEditorSpawnSpec {
    /// None → `RegionKeyAllocator::allocate()` で払い出す。
    pub region_key: Option<String>,
    /// None → `PendingStrategyFragments` から drain。
    /// Some("") → 明示的な空白 spawn。
    /// Some(s) → そのまま使う。
    pub source: Option<String>,
    pub layout_source: PanelSpawnSource,
}

// ─── Phase 7.5a: Instrument 寿命連動 ──────────────────────────────────────

/// scenario JSON `scenario.instruments` を Bevy 側で保持する registry。
/// 表示順を維持しつつ dedup する。`editable=false` のときは
/// `instruments_ref` を持つ sidecar により編集ロック中であることを示す。
#[derive(Resource, Default, Debug, Clone)]
pub struct InstrumentRegistry {
    pub ids: Vec<String>,
    pub editable: bool,
}

impl InstrumentRegistry {
    /// 新しい id を末尾に追加する。既に含まれていれば false。
    pub fn add(&mut self, id: &str) -> bool {
        if self.contains(id) {
            return false;
        }
        self.ids.push(id.to_string());
        true
    }

    /// id を取り除く。存在しなければ false。
    pub fn remove(&mut self, id: &str) -> bool {
        if let Some(pos) = self.ids.iter().position(|s| s == id) {
            self.ids.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|s| s == id)
    }

    /// registry の中身を新しい id 列で置き換える。順序は引数のまま、重複は最初の出現だけ残す。
    /// Returns true if the contents actually changed.
    pub fn replace_all(&mut self, ids: &[String]) -> bool {
        let mut deduped: Vec<String> = Vec::with_capacity(ids.len());
        for id in ids {
            if !deduped.iter().any(|s| s == id) {
                deduped.push(id.clone());
            }
        }
        if deduped == self.ids {
            false
        } else {
            self.ids = deduped;
            true
        }
    }

    /// 現在の id 列を slice で借りる（writeback / 比較用）。
    pub fn as_slice(&self) -> &[String] {
        &self.ids
    }
}

/// `parse_scenario_system` がサイドカー JSON の読み込み成功時のみ発火する。
/// registry → JSON writeback 経路では発火させない（同期方向の一方向化）。
#[derive(Event, Debug, Clone)]
pub struct ScenarioLoadedFromFile {
    pub source_path: PathBuf,
    pub instruments: Vec<String>,
    pub end: Option<String>,
    pub has_instruments_ref: bool,
}

/// `parse_scenario_system` の Local だった `last_path` / `last_mtime` を
/// Resource に格上げしたもの。writeback 後に `last_mtime` を転記して
/// 不要な再 trigger を抑止する（計画書 R5）。
#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioFileWatchState {
    pub last_path: Option<PathBuf>,
    pub last_mtime: Option<SystemTime>,
}

/// Chart window の `WindowRoot` に貼るマーカー。
/// close observer 内で逆引きして `InstrumentRegistry::remove` に渡す。
#[derive(Component, Debug, Clone)]
pub struct ChartInstrument {
    pub instrument_id: String,
}

/// writeback system と Run 直前 inline flush の dirty/flush 管理。
/// `is_changed()` の race を避けるため明示 revision を使う。
#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioInstrumentsWritebackState {
    pub revision: u64,
    pub flushed_revision: u64,
    pub last_error: Option<String>,
}

#[cfg(test)]
mod instrument_registry_tests {
    use super::*;

    #[test]
    fn test_add_appends_when_absent() {
        let mut r = InstrumentRegistry::default();
        assert!(r.add("1301.TSE"));
        assert_eq!(r.ids, vec!["1301.TSE".to_string()]);
    }

    #[test]
    fn test_add_dedup_returns_false() {
        let mut r = InstrumentRegistry::default();
        assert!(r.add("1301.TSE"));
        assert!(!r.add("1301.TSE"));
        assert_eq!(r.ids.len(), 1);
    }

    #[test]
    fn test_add_preserves_order() {
        let mut r = InstrumentRegistry::default();
        r.add("A");
        r.add("B");
        r.add("C");
        assert_eq!(r.ids, vec!["A".to_string(), "B".to_string(), "C".to_string()]);
    }

    #[test]
    fn test_remove_existing_returns_true() {
        let mut r = InstrumentRegistry::default();
        r.add("A");
        r.add("B");
        assert!(r.remove("A"));
        assert_eq!(r.ids, vec!["B".to_string()]);
    }

    #[test]
    fn test_remove_absent_returns_false() {
        let mut r = InstrumentRegistry::default();
        r.add("A");
        assert!(!r.remove("Z"));
        assert_eq!(r.ids, vec!["A".to_string()]);
    }

    #[test]
    fn test_contains() {
        let mut r = InstrumentRegistry::default();
        r.add("A");
        assert!(r.contains("A"));
        assert!(!r.contains("B"));
    }

    #[test]
    fn test_default_is_not_editable() {
        let r = InstrumentRegistry::default();
        assert!(!r.editable);
        assert!(r.ids.is_empty());
    }

    #[test]
    fn test_replace_all_dedups_preserving_order() {
        let mut reg = InstrumentRegistry::default();
        let changed = reg.replace_all(&[
            "AAPL".to_string(),
            "MSFT".to_string(),
            "AAPL".to_string(),
        ]);
        assert!(changed);
        assert_eq!(reg.as_slice(), &["AAPL".to_string(), "MSFT".to_string()]);
    }

    #[test]
    fn test_replace_all_returns_false_when_identical() {
        let mut reg = InstrumentRegistry::default();
        reg.replace_all(&["AAPL".to_string()]);
        let changed = reg.replace_all(&["AAPL".to_string()]);
        assert!(!changed);
    }
}

/// `ScenarioLoadedFromFile` を受け、registry を JSON 由来の内容で置き換える。
/// `editable = !has_instruments_ref`。ファイル由来の代入は writeback の
/// revision を flushed と同値に保ち、Run 直前 inline flush を起動させない（計画書 §3.2）。
pub fn sync_registry_from_scenario_loaded_system(
    mut events: EventReader<ScenarioLoadedFromFile>,
    mut registry: ResMut<InstrumentRegistry>,
    mut writeback: ResMut<ScenarioInstrumentsWritebackState>,
) {
    for ev in events.read() {
        registry.replace_all(&ev.instruments);
        registry.editable = !ev.has_instruments_ref;
        writeback.revision = writeback.flushed_revision;
        writeback.last_error = None;
    }
}

#[cfg(test)]
mod sync_registry_from_scenario_loaded_tests {
    use super::*;

    fn build_app() -> App {
        let mut app = App::new();
        app.add_event::<ScenarioLoadedFromFile>()
            .init_resource::<InstrumentRegistry>()
            .init_resource::<ScenarioInstrumentsWritebackState>()
            .add_systems(Update, sync_registry_from_scenario_loaded_system);
        app
    }

    #[test]
    fn replaces_and_marks_locked_when_has_instruments_ref_true() {
        let mut app = build_app();
        app.world_mut().send_event(ScenarioLoadedFromFile {
            source_path: std::path::PathBuf::from("dummy.py"),
            instruments: vec!["7203.T".into(), "9984.T".into()],
            end: None,
            has_instruments_ref: true,
        });
        app.update();
        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.as_slice(), &["7203.T".to_string(), "9984.T".to_string()]);
        assert!(!reg.editable, "instruments_ref ありは編集ロック");
    }

    #[test]
    fn replaces_and_marks_editable_when_has_instruments_ref_false() {
        let mut app = build_app();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["AAPL".to_string()]);
        }
        app.world_mut().send_event(ScenarioLoadedFromFile {
            source_path: std::path::PathBuf::from("dummy.py"),
            instruments: vec!["7203.T".into()],
            end: None,
            has_instruments_ref: false,
        });
        app.update();
        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.as_slice(), &["7203.T".to_string()]);
        assert!(reg.editable, "instruments_ref 無しは編集可");
    }

    #[test]
    fn file_load_does_not_bump_writeback_revision() {
        let mut app = build_app();
        app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>().flushed_revision = 5;
        app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>().revision = 5;
        app.world_mut().send_event(ScenarioLoadedFromFile {
            source_path: std::path::PathBuf::from("dummy.py"),
            instruments: vec!["7203.T".into()],
            end: None,
            has_instruments_ref: false,
        });
        app.update();
        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(wb.revision, wb.flushed_revision, "ファイル由来は flushed と同値に保つ");
        assert!(wb.last_error.is_none());
    }
}

/// `InstrumentRegistry` の change detection を見て `writeback.revision` を +1 する。
/// ファイルロード由来の代入は §3.2 の `sync_registry_from_scenario_loaded_system` が
/// 同 tick 内で `revision = flushed_revision` に戻すため、ここで inc されてもループしない
/// (writeback 成功時に再び `flushed_revision = revision` で追随する)。計画書 §3.3。
pub fn mark_registry_dirty_system(
    registry: Res<InstrumentRegistry>,
    mut writeback: ResMut<ScenarioInstrumentsWritebackState>,
) {
    if !registry.is_changed() {
        return;
    }
    writeback.revision += 1;
}

#[cfg(test)]
mod mark_registry_dirty_tests {
    use super::*;

    fn build_app() -> App {
        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>()
            .init_resource::<ScenarioInstrumentsWritebackState>()
            .add_systems(Update, mark_registry_dirty_system);
        app
    }

    #[test]
    fn test_registry_change_increments_writeback_revision() {
        let mut app = build_app();
        app.update();
        let baseline = app.world().resource::<ScenarioInstrumentsWritebackState>().revision;

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["7203.T".to_string()]);
        }
        app.update();

        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(wb.revision, baseline + 1, "registry mutate で revision が +1");
        assert_eq!(wb.flushed_revision, 0, "flushed_revision は据え置き");
    }

    #[test]
    fn test_unchanged_registry_does_not_increment() {
        let mut app = build_app();
        app.update();
        let baseline = app.world().resource::<ScenarioInstrumentsWritebackState>().revision;

        app.update();
        app.update();

        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(wb.revision, baseline, "registry 未変更なら inc しない");
    }
}

// ─── Phase 7.5b: scenario.instruments writeback ───────────────────────────

/// scenario.instruments の永続化 target path 群。
/// `cache_sidecar` = `cache_state_paths().0`（= `<cache>/app_state.json`）。
/// 元 sidecar path は `StrategyBuffer.original_path` から `.with_extension("json")` で導出するので
/// ここには持たない。production では `UiPlugin` 起動時に挿入する。
#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioWritebackPaths {
    pub cache_sidecar: Option<PathBuf>,
}

/// registry が dirty (`revision != flushed_revision`) かつ `editable == true` のときに、
/// 元 sidecar (`<original_path stem>.json`) と cache sidecar (`paths.cache_sidecar`) の
/// `scenario.instruments` だけを registry.ids で置換する。
pub fn writeback_scenario_instruments_system(
    registry: Res<InstrumentRegistry>,
    _buffer: Res<StrategyBuffer>,
    paths: Res<ScenarioWritebackPaths>,
    mut writeback: ResMut<ScenarioInstrumentsWritebackState>,
    mut watch: ResMut<ScenarioFileWatchState>,
) {
    if !registry.editable {
        return;
    }
    if writeback.revision == writeback.flushed_revision {
        return;
    }

    match flush_sidecars_now(
        registry.as_slice(),
        None,
        paths.cache_sidecar.as_deref(),
    ) {
        Ok(new_mtime) => {
            writeback.flushed_revision = writeback.revision;
            writeback.last_error = None;
            if let Some(m) = new_mtime {
                watch.last_mtime = Some(m);
            }
        }
        Err(msg) => {
            writeback.last_error = Some(msg);
        }
    }
}

/// 計画書 KC2: registry 編集を ScenarioMetadata.instruments に直接同期する。
/// `writeback_scenario_instruments_system` と同じ revision dirty ゲート
/// (registry.editable && revision != flushed_revision) で起動する。
/// scenario.instruments と registry が同値なら no-op
/// (ScenarioMetadata の change detection を毎 tick 汚さないため)。
pub fn sync_scenario_metadata_from_registry_system(
    registry: Res<InstrumentRegistry>,
    writeback: Res<ScenarioInstrumentsWritebackState>,
    mut scenario: ResMut<ScenarioMetadata>,
) {
    if !registry.editable {
        return;
    }
    if writeback.revision == writeback.flushed_revision {
        return;
    }
    let new_ids = registry.as_slice();
    if scenario.instruments.as_slice() == new_ids {
        return;
    }
    scenario.instruments = new_ids.to_vec();
}

/// 計画書 §3.4 / §3.5: writeback 本体ロジックを system 外から呼べる pure 関数として切り出したもの。
/// `writeback_scenario_instruments_system` (revision ベース dirty 判定) と
/// `handle_strategy_run_system` 内 inline flush (revision 無関係に「今書く」) の両方が共有する。
///
/// 戻り値: `Ok(Some(mtime))` = 元 sidecar の write 成功時の新 mtime（watch state 転記用）。
///         `Ok(None)`         = cache sidecar のみ書いた、または元 sidecar が無い。
///         `Err(msg)`         = いずれかの target で失敗。`last_error` 用文字列。
///
/// `editable == false` のときは呼び出し側で skip する想定（この関数は ref チェックしない）。
pub fn flush_sidecars_now(
    registry_ids: &[String],
    original_py: Option<&std::path::Path>,
    cache_sidecar: Option<&std::path::Path>,
) -> Result<Option<SystemTime>, String> {
    let mut targets: Vec<PathBuf> = Vec::with_capacity(2);
    if let Some(p) = original_py {
        targets.push(p.with_extension("json"));
    }
    if let Some(p) = cache_sidecar {
        if !targets.iter().any(|t| t == p) {
            targets.push(p.to_path_buf());
        }
    }
    if targets.is_empty() {
        return Err("no writeback target".to_string());
    }

    let new_ids: Vec<serde_json::Value> = registry_ids
        .iter()
        .map(|s| serde_json::Value::String(s.clone()))
        .collect();

    let mut original_mtime: Option<SystemTime> = None;
    for path in &targets {
        if let Err(e) = rewrite_scenario_instruments_atomic(path, &new_ids) {
            return Err(format!("writeback {path:?}: {e}"));
        }
        if let Some(orig_py) = original_py {
            if path == &orig_py.with_extension("json") {
                if let Ok(meta) = std::fs::metadata(path) {
                    if let Ok(m) = meta.modified() {
                        original_mtime = Some(m);
                    }
                }
            }
        }
    }
    Ok(original_mtime)
}

fn rewrite_scenario_instruments_atomic(
    path: &std::path::Path,
    new_ids: &[serde_json::Value],
) -> std::io::Result<()> {
    let raw = crate::ui::layout_persistence::read_json_with_bom_strip(path)?;
    let mut value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    {
        let scenario = value
            .get_mut("scenario")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "missing scenario object",
                )
            })?;

        // v1 → v2 正規化:
        // - schema_version が None または 1 で、legacy `instrument` キー（単数 or list）がある場合、
        //   `instrument` を削除して `schema_version=2` にセットする。
        let needs_normalize = {
            let ver = scenario.get("schema_version").and_then(|v| v.as_u64());
            let has_legacy_instrument = scenario.contains_key("instrument");
            has_legacy_instrument && matches!(ver, None | Some(1))
        };
        if needs_normalize {
            scenario.remove("instrument");
            scenario.insert(
                "schema_version".to_string(),
                serde_json::Value::Number(2u64.into()),
            );
            warn!("normalized v1 sidecar to v2: {:?}", path);
        }

        scenario.insert(
            "instruments".to_string(),
            serde_json::Value::Array(new_ids.to_vec()),
        );
    }

    let serialized = serde_json::to_string_pretty(&value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no filename")
        })?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::write(&tmp, serialized.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod writeback_scenario_instruments_tests {
    use super::*;
    use std::fs;

    /// 計画書 §3.4 / §5.1「永続化」最上段:
    /// registry が dirty(=revision != flushed_revision) の状態で
    /// `writeback_scenario_instruments_system` を 1 tick 回すと、
    /// **元 sidecar と cache sidecar の両方** の `scenario.instruments` が
    /// registry.ids で置換される。
    #[test]
    fn test_writeback_updates_cache_only_preserves_original() {
        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_json = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["OLD.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&cache_json, initial).unwrap();
        let original_before = fs::read(&original_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["1301.TSE".to_string(), "7203.TSE".to_string()]);
            reg.editable = true;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 1;
            wb.flushed_revision = 0;
        }
        app.add_systems(Update, writeback_scenario_instruments_system);
        app.update();

        assert_eq!(
            fs::read(&original_json).unwrap(),
            original_before,
            "original sidecar must NOT be touched (CacheOnly policy)"
        );
        let updated_cache: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_json).unwrap()).unwrap();
        assert_eq!(
            updated_cache["scenario"]["instruments"],
            serde_json::json!(["1301.TSE", "7203.TSE"]),
            "cache sidecar must reflect new instruments"
        );

        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(wb.flushed_revision, wb.revision, "flushed_revision must catch up on success");
        assert!(wb.last_error.is_none());
    }

    /// 計画書 §5.1「永続化」: `scenario.instruments` 以外のキー
    /// (start / end / granularity / initial_cash / schema_version) は
    /// 1 文字も変えない。
    #[test]
    fn test_writeback_only_touches_scenario_instruments_field() {
        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_json = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario":{"schema_version":2,"instruments":["OLD.T"],"start":"2025-01-06","end":"2025-01-10","granularity":"Daily","initial_cash":1000000,"custom_extra":"keep-me"},"viewport":{"x":42},"windows":[{"id":"w1"}]}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&cache_json, initial).unwrap();
        let original_before = fs::read(&original_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["NEW.T".to_string()]);
            reg.editable = true;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 1;
            wb.flushed_revision = 0;
        }
        app.add_systems(Update, writeback_scenario_instruments_system);
        app.update();

        assert_eq!(
            fs::read(&original_json).unwrap(),
            original_before,
            "original sidecar must not be touched in CacheOnly mode"
        );

        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_json).unwrap()).unwrap();

        assert_eq!(updated["scenario"]["instruments"], serde_json::json!(["NEW.T"]));
        assert_eq!(updated["scenario"]["schema_version"], serde_json::json!(2));
        assert_eq!(updated["scenario"]["start"], serde_json::json!("2025-01-06"));
        assert_eq!(updated["scenario"]["end"], serde_json::json!("2025-01-10"));
        assert_eq!(updated["scenario"]["granularity"], serde_json::json!("Daily"));
        assert_eq!(updated["scenario"]["initial_cash"], serde_json::json!(1000000));
        assert_eq!(updated["scenario"]["custom_extra"], serde_json::json!("keep-me"));
        assert_eq!(updated["viewport"], serde_json::json!({"x": 42}));
        assert_eq!(updated["windows"], serde_json::json!([{"id": "w1"}]));
    }

    /// 計画書 §5.1「Schema 互換」:
    /// v1 単数 `instrument: "1301.TSE"` を持つ sidecar に対し registry に
    /// 銘柄が入った状態で writeback → `schema_version=2`,
    /// `instrument` キー削除, `instruments: [...]` 書き込み。
    #[test]
    fn test_writeback_normalizes_v1_to_v2() {
        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_json = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario":{"schema_version":1,"instrument":"1301.TSE","start":"2025-01-06","end":"2025-01-10","granularity":"Daily","initial_cash":1000000}}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&cache_json, initial).unwrap();
        let original_before = fs::read(&original_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["1301.TSE".to_string(), "7203.TSE".to_string()]);
            reg.editable = true;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 1;
            wb.flushed_revision = 0;
        }
        app.add_systems(Update, writeback_scenario_instruments_system);
        app.update();

        assert_eq!(
            fs::read(&original_json).unwrap(),
            original_before,
            "original sidecar must not be touched in CacheOnly mode"
        );

        let cache: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_json).unwrap()).unwrap();
        assert_eq!(cache["scenario"]["schema_version"], serde_json::json!(2));
        assert_eq!(
            cache["scenario"]["instruments"],
            serde_json::json!(["1301.TSE", "7203.TSE"])
        );
        assert!(cache["scenario"].get("instrument").is_none());
    }

    /// 計画書 §5.1「Schema 互換」:
    /// legacy で `instrument: ["A","B"]`（list 形式）の sidecar も
    /// v2 正規化される。
    #[test]
    fn test_writeback_handles_legacy_instrument_as_list() {
        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_json = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario":{"instrument":["A","B"],"start":"2025-01-06","end":"2025-01-10","granularity":"Daily","initial_cash":1000000}}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&cache_json, initial).unwrap();
        let original_before = fs::read(&original_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["NEW.T".to_string()]);
            reg.editable = true;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 1;
            wb.flushed_revision = 0;
        }
        app.add_systems(Update, writeback_scenario_instruments_system);
        app.update();

        assert_eq!(
            fs::read(&original_json).unwrap(),
            original_before,
            "original sidecar must not be touched in CacheOnly mode"
        );

        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_json).unwrap()).unwrap();

        assert_eq!(updated["scenario"]["schema_version"], serde_json::json!(2));
        assert_eq!(updated["scenario"]["instruments"], serde_json::json!(["NEW.T"]));
        assert!(
            updated["scenario"].get("instrument").is_none(),
            "legacy 'instrument' key (list form) must be removed"
        );
    }

    /// 計画書 §3.3: `registry.editable = false`（= instruments_ref ロック中）の間は
    /// dirty(revision != flushed_revision) でも writeback system は no-op。
    /// 元 sidecar / cache sidecar の bytes は不変で、`flushed_revision` も据え置き。
    #[test]
    fn test_instruments_ref_locks_writeback() {
        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_json = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["LOCKED.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&cache_json, initial).unwrap();

        let original_bytes = fs::read(&original_json).unwrap();
        let cache_bytes = fs::read(&cache_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["NEW.T".to_string()]);
            reg.editable = false;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 7;
            wb.flushed_revision = 0;
        }
        app.add_systems(Update, writeback_scenario_instruments_system);
        app.update();

        assert_eq!(fs::read(&original_json).unwrap(), original_bytes, "locked: original must be byte-identical");
        assert_eq!(fs::read(&cache_json).unwrap(), cache_bytes, "locked: cache must be byte-identical");

        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(wb.flushed_revision, 0, "locked: flushed_revision must stay");
        assert!(wb.last_error.is_none(), "locked: no error path");
    }

    /// 計画書 §3.4: writeback target が書き込み不能だと `flushed_revision` は据え置きで
    /// `last_error` がセットされ、次フレームで有効 path に差し替えれば自動再試行が成功する。
    #[test]
    fn test_writeback_failure_keeps_revision_and_retries() {
        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let bad_cache = dir.path().join("nonexistent_subdir").join("app_state.json");
        let good_cache = dir.path().join("app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["OLD.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&good_cache, initial).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(bad_cache.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["RETRY.T".to_string()]);
            reg.editable = true;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 3;
            wb.flushed_revision = 0;
        }
        app.add_systems(Update, writeback_scenario_instruments_system);

        app.update();
        {
            let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
            assert_eq!(wb.flushed_revision, 0, "failure: flushed_revision must stay");
            assert!(wb.last_error.is_some(), "failure: last_error must be set");
        }

        {
            let mut paths = app.world_mut().resource_mut::<ScenarioWritebackPaths>();
            paths.cache_sidecar = Some(good_cache.clone());
        }
        app.update();

        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(wb.flushed_revision, wb.revision, "retry: flushed_revision must catch up");
        assert!(wb.last_error.is_none(), "retry: last_error must clear on success");

        let updated_good: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&good_cache).unwrap()).unwrap();
        assert_eq!(
            updated_good["scenario"]["instruments"],
            serde_json::json!(["RETRY.T"]),
            "retry: good cache sidecar must reflect new instruments"
        );
    }

    /// 計画書 §3.4: writeback 成功時に `ScenarioFileWatchState.last_mtime` を新 mtime に
    /// 転記しているため、次 tick で `parse_scenario_system` を回しても
    /// 自分が書いた sidecar を「外部変更」と誤検知せず `ScenarioLoadedFromFile` を発火しない。
    #[test]
    fn test_writeback_does_not_retrigger_scenario_reload() {
        use crate::ui::components::{ScenarioLoadedFromFile, ScenarioMetadata};
        use crate::ui::scenario_parser::parse_scenario_system;

        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["OLD.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, initial).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: None,
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.init_resource::<ScenarioFileWatchState>();
        app.init_resource::<ScenarioMetadata>();
        app.add_event::<ScenarioLoadedFromFile>();

        app.add_systems(Update, parse_scenario_system);
        app.update();
        app.world_mut().resource_mut::<Events<ScenarioLoadedFromFile>>().clear();

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["NEW.T".to_string()]);
            reg.editable = true;
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 1;
            wb.flushed_revision = 0;
        }

        app.add_systems(Update, (writeback_scenario_instruments_system, parse_scenario_system).chain());
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut cursor = events.get_cursor();
        let collected: Vec<_> = cursor.read(events).collect();
        assert!(
            collected.is_empty(),
            "writeback must transcribe last_mtime so parse_scenario_system does not refire ScenarioLoadedFromFile (got {} events)",
            collected.len()
        );
    }

    /// 計画書 §3.5 / §5.1: `handle_strategy_run_system` は RunStrategy 送信直前に
    /// registry.editable && flush_sidecars_now() を実行する。
    #[test]
    fn test_run_inline_flush_writes_cache_only_preserves_original() {
        use crate::trading::{TransportCommand, TransportCommandSender};
        use crate::ui::components::StrategyRunRequested;
        use crate::ui::menu_bar::handle_strategy_run_system;

        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_sidecar = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["OLD.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, initial).unwrap();
        fs::write(&cache_sidecar, initial).unwrap();
        let original_before = fs::read(&original_json).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();

        let mut app = App::new();
        app.add_event::<StrategyRunRequested>();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_sidecar.clone()),
        });
        app.insert_resource(TransportCommandSender { tx });
        app.init_resource::<InstrumentRegistry>();
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["A.T".to_string(), "B.T".to_string()],
            start: Some("2025-01-06".to_string()),
            end: Some("2025-01-10".to_string()),
            granularity: Some("Daily".to_string()),
            initial_cash: Some(1_000_000),
            ..Default::default()
        });
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string(), "B.T".to_string()]);
            reg.editable = true;
        }

        app.add_systems(Update, handle_strategy_run_system);

        app.world_mut().send_event(StrategyRunRequested {
            cache_path: original_py.clone(),
        });
        app.update();

        assert_eq!(
            fs::read(&original_json).unwrap(),
            original_before,
            "original sidecar must NOT be touched (CacheOnly policy)"
        );
        let updated_cache: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_sidecar).unwrap()).unwrap();
        assert_eq!(
            updated_cache["scenario"]["instruments"],
            serde_json::json!(["A.T", "B.T"]),
        );

        let cmd = rx.try_recv().expect("RunStrategy must be sent after flush");
        match cmd {
            TransportCommand::RunStrategy { strategy_file, .. } => {
                assert_eq!(strategy_file, original_py);
            }
            other => panic!("expected RunStrategy, got {:?}", other),
        }
    }

    /// 計画書 §3.5: inline flush は idempotent。
    #[test]
    fn test_run_inline_flush_is_idempotent() {
        use crate::trading::{TransportCommand, TransportCommandSender};
        use crate::ui::components::StrategyRunRequested;
        use crate::ui::menu_bar::handle_strategy_run_system;

        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        let cache_sidecar = dir.path().join("cache_app_state.json");

        fs::write(&original_py, "# dummy").unwrap();
        let already_flushed = r#"{"scenario": {"schema_version": 2, "instruments": ["A.T", "B.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, already_flushed).unwrap();
        fs::write(&cache_sidecar, already_flushed).unwrap();
        let original_before = fs::read(&original_json).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();

        let mut app = App::new();
        app.add_event::<StrategyRunRequested>();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_sidecar.clone()),
        });
        app.insert_resource(TransportCommandSender { tx });
        app.init_resource::<InstrumentRegistry>();
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["A.T".to_string(), "B.T".to_string()],
            start: Some("2025-01-06".to_string()),
            end: Some("2025-01-10".to_string()),
            granularity: Some("Daily".to_string()),
            initial_cash: Some(1_000_000),
            ..Default::default()
        });
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string(), "B.T".to_string()]);
            reg.editable = true;
        }

        app.add_systems(Update, handle_strategy_run_system);

        app.world_mut().send_event(StrategyRunRequested {
            cache_path: original_py.clone(),
        });
        app.update();
        let _ = rx.try_recv().expect("first RunStrategy must be sent");

        app.world_mut().send_event(StrategyRunRequested {
            cache_path: original_py.clone(),
        });
        app.update();
        let cmd = rx.try_recv().expect("second RunStrategy must be sent (inline flush is idempotent)");
        match cmd {
            TransportCommand::RunStrategy { strategy_file, .. } => {
                assert_eq!(strategy_file, original_py);
            }
            other => panic!("expected RunStrategy on second run, got {:?}", other),
        }

        assert_eq!(
            fs::read(&original_json).unwrap(),
            original_before,
            "original sidecar must NOT be touched (CacheOnly policy)"
        );
        let updated_cache: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_sidecar).unwrap()).unwrap();
        assert_eq!(
            updated_cache["scenario"]["instruments"],
            serde_json::json!(["A.T", "B.T"]),
        );
    }

    /// 計画書 §3.5: inline flush 失敗時は RunStrategy 未送信、event は消費。
    #[test]
    fn test_run_blocked_when_inline_flush_fails() {
        use crate::trading::{TransportCommand, TransportCommandSender};
        use crate::ui::components::StrategyRunRequested;
        use crate::ui::menu_bar::handle_strategy_run_system;

        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        fs::write(&original_py, "# dummy").unwrap();
        // sibling json は作らない → read_to_string で NotFound → flush_sidecars_now が Err

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();

        let mut app = App::new();
        app.add_event::<StrategyRunRequested>();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: None,
        });
        app.insert_resource(TransportCommandSender { tx });
        app.init_resource::<InstrumentRegistry>();
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["A.T".to_string()],
            start: Some("2025-01-06".to_string()),
            end: Some("2025-01-10".to_string()),
            granularity: Some("Daily".to_string()),
            initial_cash: Some(1_000_000),
            ..Default::default()
        });
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string()]);
            reg.editable = true;
        }

        app.add_systems(Update, handle_strategy_run_system);

        app.world_mut().send_event(StrategyRunRequested {
            cache_path: original_py.clone(),
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "RunStrategy must NOT be sent when inline flush fails"
        );

        app.update();
        assert!(
            rx.try_recv().is_err(),
            "event must be drained even on flush failure (no replay on next tick)"
        );
    }

    /// 計画書 §3.5 退化検知: handler は registry.is_changed() ガードを入れない。
    #[test]
    fn test_run_does_not_use_is_changed_guard() {
        use crate::trading::{TransportCommand, TransportCommandSender};
        use crate::ui::components::StrategyRunRequested;
        use crate::ui::menu_bar::handle_strategy_run_system;

        let dir = tempfile::tempdir().unwrap();
        let original_py = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        fs::write(&original_py, "# dummy").unwrap();
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["OLD.T"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        fs::write(&original_json, initial).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();

        let mut app = App::new();
        app.add_event::<StrategyRunRequested>();
        app.insert_resource(StrategyBuffer {
            original_path: Some(original_py.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(original_json.clone()),
        });
        app.insert_resource(TransportCommandSender { tx });
        app.init_resource::<InstrumentRegistry>();
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["NEW.T".to_string()],
            start: Some("2025-01-06".to_string()),
            end: Some("2025-01-10".to_string()),
            granularity: Some("Daily".to_string()),
            initial_cash: Some(1_000_000),
            ..Default::default()
        });

        app.add_systems(Update, handle_strategy_run_system);

        app.update();

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["NEW.T".to_string()]);
            reg.editable = true;
        }
        app.world_mut().send_event(StrategyRunRequested {
            cache_path: original_py.clone(),
        });
        app.update();

        let cmd = rx.try_recv().expect(
            "RunStrategy must be sent even when registry was just mutated in the same tick",
        );
        match cmd {
            TransportCommand::RunStrategy { strategy_file, .. } => {
                assert_eq!(strategy_file, original_py);
            }
            other => panic!("expected RunStrategy, got {:?}", other),
        }
    }

    /// 計画書 §5.2 E2E-1: StrategyFileLoadRequested(scenario-only sidecar) を投げると、
    /// parse_scenario_system → ScenarioLoadedFromFile → sync_registry_from_scenario_loaded_system
    /// → instrument_chart_sync_system まで通り、InstrumentRegistry.ids と Chart entity 2 つが揃う。
    #[test]
    fn test_e2e_open_to_chart_spawn() {
        use crate::ui::components::{
            ChartInstrument, InstrumentRegistry, PendingStrategyFragments, RegionKeyAllocator,
            ScenarioFileWatchState, ScenarioInstrumentsWritebackState, ScenarioLoadedFromFile,
            ScenarioMetadata, StrategyFileLoadRequested, StrategyLoadMode, WindowRoot,
            sync_registry_from_scenario_loaded_system,
        };
        use crate::ui::layout_persistence::LayoutLoadRequested;
        use crate::ui::menu_bar::handle_strategy_file_load_system;
        use crate::ui::scenario_parser::parse_scenario_system;
        use crate::ui::window::instrument_chart_sync_system;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("pair_trade_minute.py");
        let json_path = dir.path().join("pair_trade_minute.json");
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Minute", "initial_cash": 1000000}}"#,
        ).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: None,
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.init_resource::<RegionKeyAllocator>();
        app.init_resource::<PendingStrategyFragments>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.add_event::<StrategyFileLoadRequested>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();

        app.add_systems(
            Update,
            (
                handle_strategy_file_load_system,
                parse_scenario_system,
                sync_registry_from_scenario_loaded_system,
                instrument_chart_sync_system,
            ).chain(),
        );

        app.world_mut().send_event(StrategyFileLoadRequested {
            path: py_path.clone(),
            mode: StrategyLoadMode::UserOpen,
        });
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.ids, vec!["1301.TSE".to_string(), "7203.TSE".to_string()]);

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let mut ids: Vec<String> = q.iter(world).map(|c| c.instrument_id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["1301.TSE".to_string(), "7203.TSE".to_string()]);
    }

    /// 計画書 §5.2 E2E-2: open 後に registry から 1 件 close（直接 mutate）すると、
    /// mark_registry_dirty_system → writeback_scenario_instruments_system が走り、
    /// Chart entity の despawn と両 sidecar JSON の `scenario.instruments` 更新が連動する。
    /// close observer 配送経路は本 test の範囲外で、registry の mutate で代替する。
    #[test]
    fn test_e2e_close_writeback() {
        use crate::ui::components::{
            ChartInstrument, InstrumentRegistry, PendingStrategyFragments, RegionKeyAllocator,
            ScenarioFileWatchState, ScenarioInstrumentsWritebackState, ScenarioLoadedFromFile,
            ScenarioMetadata, ScenarioWritebackPaths, StrategyFileLoadRequested, StrategyLoadMode,
            WindowRoot, mark_registry_dirty_system, sync_registry_from_scenario_loaded_system,
            writeback_scenario_instruments_system,
        };
        use crate::ui::layout_persistence::LayoutLoadRequested;
        use crate::ui::menu_bar::handle_strategy_file_load_system;
        use crate::ui::scenario_parser::parse_scenario_system;
        use crate::ui::window::instrument_chart_sync_system;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("pair_trade_minute.py");
        let json_path = dir.path().join("pair_trade_minute.json");
        let cache_json_path = dir.path().join("app_state.json");
        let initial_json = r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Minute", "initial_cash": 1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial_json).unwrap();
        std::fs::write(&cache_json_path, initial_json).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: None,
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.init_resource::<RegionKeyAllocator>();
        app.init_resource::<PendingStrategyFragments>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.add_event::<StrategyFileLoadRequested>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();

        app.add_systems(
            Update,
            (
                handle_strategy_file_load_system,
                parse_scenario_system,
                sync_registry_from_scenario_loaded_system,
                instrument_chart_sync_system,
                mark_registry_dirty_system,
                writeback_scenario_instruments_system,
            ).chain(),
        );

        app.world_mut().send_event(StrategyFileLoadRequested {
            path: py_path.clone(),
            mode: StrategyLoadMode::UserOpen,
        });
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.ids, vec!["1301.TSE".to_string(), "7203.TSE".to_string()]);
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
            assert_eq!(q.iter(world).count(), 2, "1 tick 目で Chart 2 件");
        }

        // close 相当: registry から 7203.TSE を除去
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.ids.retain(|id| id != "7203.TSE");
        }
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.ids, vec!["1301.TSE".to_string()]);

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let ids: Vec<String> = q.iter(world).map(|c| c.instrument_id.clone()).collect();
        assert_eq!(ids, vec!["1301.TSE".to_string()], "Chart 1 件で 1301.TSE のみ");

        // original sidecar は CacheOnly 仕様で据え置き
        {
            let body = std::fs::read_to_string(&json_path).unwrap();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            let instruments = v["scenario"]["instruments"].as_array().unwrap();
            let got: Vec<String> = instruments.iter().map(|x| x.as_str().unwrap().to_string()).collect();
            assert_eq!(
                got,
                vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
                "original sidecar ({:?}) は CacheOnly で据え置き", json_path
            );
        }
        // cache sidecar だけ新 registry を反映
        {
            let body = std::fs::read_to_string(&cache_json_path).unwrap();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            let instruments = v["scenario"]["instruments"].as_array().unwrap();
            let got: Vec<String> = instruments.iter().map(|x| x.as_str().unwrap().to_string()).collect();
            assert_eq!(got, vec!["1301.TSE".to_string()], "cache sidecar が縮んでいる");
        }
    }

    /// 計画書 §5.2 E2E-3: open → close mutate → StrategyRunRequested まで一気通貫で、
    /// `handle_strategy_run_system` の inline flush が走り、TransportCommand::RunStrategy が
    /// 新 instruments を反映した sidecar を伴って送出される。
    #[test]
    fn test_e2e_close_and_run_uses_new_instruments() {
        use crate::trading::{TransportCommand, TransportCommandSender};
        use crate::ui::components::{
            ChartInstrument, InstrumentRegistry, PendingStrategyFragments, RegionKeyAllocator,
            ScenarioFileWatchState, ScenarioInstrumentsWritebackState, ScenarioLoadedFromFile,
            ScenarioMetadata, ScenarioWritebackPaths, StrategyFileLoadRequested, StrategyLoadMode,
            StrategyRunRequested, WindowRoot, mark_registry_dirty_system,
            sync_registry_from_scenario_loaded_system, writeback_scenario_instruments_system,
        };
        use crate::ui::layout_persistence::LayoutLoadRequested;
        use crate::ui::menu_bar::{handle_strategy_file_load_system, handle_strategy_run_system};
        use crate::ui::scenario_parser::parse_scenario_system;
        use crate::ui::window::instrument_chart_sync_system;

        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("pair_trade_minute.py");
        let json_path = dir.path().join("pair_trade_minute.json");
        let cache_json_path = dir.path().join("app_state.json");
        let initial_json = r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Minute", "initial_cash": 1000000}}"#;
        std::fs::write(&py_path, "# dummy\n").unwrap();
        std::fs::write(&json_path, initial_json).unwrap();
        std::fs::write(&cache_json_path, initial_json).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: None,
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_json_path.clone()),
        });
        app.insert_resource(TransportCommandSender { tx });
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.init_resource::<RegionKeyAllocator>();
        app.init_resource::<PendingStrategyFragments>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.add_event::<StrategyFileLoadRequested>();
        app.add_event::<StrategyRunRequested>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<LayoutLoadRequested>();
        app.add_event::<PanelSpawnRequested>();

        app.add_systems(
            Update,
            (
                handle_strategy_file_load_system,
                parse_scenario_system,
                sync_registry_from_scenario_loaded_system,
                instrument_chart_sync_system,
                mark_registry_dirty_system,
                writeback_scenario_instruments_system,
                handle_strategy_run_system,
            ).chain(),
        );

        app.world_mut().send_event(StrategyFileLoadRequested {
            path: py_path.clone(),
            mode: StrategyLoadMode::UserOpen,
        });
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.ids, vec!["1301.TSE".to_string(), "7203.TSE".to_string()]);
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
            assert_eq!(q.iter(world).count(), 2, "1 tick 目で Chart 2 件");
        }

        // close 相当: registry から 7203.TSE を除去 + Run 発火
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.ids.retain(|id| id != "7203.TSE");
        }
        app.world_mut().send_event(StrategyRunRequested {
            cache_path: py_path.clone(),
        });
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert_eq!(reg.ids, vec!["1301.TSE".to_string()]);

        let cmd = rx.try_recv().expect("RunStrategy must be sent after close+run");
        match cmd {
            TransportCommand::RunStrategy { strategy_file, .. } => {
                assert_eq!(strategy_file, py_path);
            }
            other => panic!("expected RunStrategy, got {:?}", other),
        }

        // original sidecar は CacheOnly 仕様で据え置き
        {
            let body = std::fs::read_to_string(&json_path).unwrap();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            let instruments = v["scenario"]["instruments"].as_array().unwrap();
            let got: Vec<String> = instruments.iter().map(|x| x.as_str().unwrap().to_string()).collect();
            assert_eq!(
                got,
                vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
                "original sidecar ({:?}) は CacheOnly で据え置き", json_path
            );
        }
        // cache sidecar だけ新 registry を反映
        {
            let body = std::fs::read_to_string(&cache_json_path).unwrap();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            let instruments = v["scenario"]["instruments"].as_array().unwrap();
            let got: Vec<String> = instruments.iter().map(|x| x.as_str().unwrap().to_string()).collect();
            assert_eq!(got, vec!["1301.TSE".to_string()], "cache sidecar が縮んでいる");
        }
    }

    #[test]
    #[ignore = "Phase 7.5a R1: Save As 経路の writeback.revision 強制 inc が Step 3 未実装。\
                handle_strategy_file_load_system 等で buffer.original_path が None→Some に\
                遷移した tick に writeback.revision += 1 する小修正が入ってから外す。"]
    fn test_e2e_save_as_after_unsaved_add() {
        // R1 修正後に本実装
        todo!("Phase 7.5a R1 修正後に本実装");
    }

    /// KC7 前提固定: editable=false なら dirty (revision != flushed) でも
    /// scenario.instruments は触らない。
    #[test]
    fn sync_scenario_metadata_from_registry_skips_when_not_editable() {
        let mut app = App::new();
        app.insert_resource(InstrumentRegistry {
            ids: vec!["NEW.T".to_string()],
            editable: false,
        });
        app.insert_resource(ScenarioInstrumentsWritebackState {
            revision: 1,
            flushed_revision: 0,
            last_error: None,
        });
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["OLD.T".to_string()],
            ..Default::default()
        });
        app.add_systems(Update, sync_scenario_metadata_from_registry_system);
        app.update();

        let scen = app.world().resource::<ScenarioMetadata>();
        assert_eq!(scen.instruments, vec!["OLD.T".to_string()]);
    }

    /// KC7 前提固定: editable=true でも revision == flushed なら no-op。
    /// (registry と scenario が乖離していても触らない)
    #[test]
    fn sync_scenario_metadata_from_registry_skips_when_revision_clean() {
        let mut app = App::new();
        app.insert_resource(InstrumentRegistry {
            ids: vec!["NEW.T".to_string()],
            editable: true,
        });
        app.insert_resource(ScenarioInstrumentsWritebackState {
            revision: 3,
            flushed_revision: 3,
            last_error: None,
        });
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["OLD.T".to_string()],
            ..Default::default()
        });
        app.add_systems(Update, sync_scenario_metadata_from_registry_system);
        app.update();

        let scen = app.world().resource::<ScenarioMetadata>();
        assert_eq!(scen.instruments, vec!["OLD.T".to_string()]);
    }

    /// KC7 前提固定: dirty かつ editable でも registry == scenario なら
    /// no-op 同値ガードで触らない。
    #[test]
    fn sync_scenario_metadata_from_registry_noop_when_already_equal() {
        let mut app = App::new();
        app.insert_resource(InstrumentRegistry {
            ids: vec!["SAME.T".to_string(), "OTHER.T".to_string()],
            editable: true,
        });
        app.insert_resource(ScenarioInstrumentsWritebackState {
            revision: 2,
            flushed_revision: 1,
            last_error: None,
        });
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["SAME.T".to_string(), "OTHER.T".to_string()],
            ..Default::default()
        });
        app.add_systems(Update, sync_scenario_metadata_from_registry_system);
        app.update();

        let scen = app.world().resource::<ScenarioMetadata>();
        assert_eq!(
            scen.instruments,
            vec!["SAME.T".to_string(), "OTHER.T".to_string()]
        );
    }

    /// KC7 前提固定: editable=true かつ dirty かつ registry != scenario なら
    /// scenario.instruments が registry に追従する。
    #[test]
    fn sync_scenario_metadata_from_registry_updates_when_dirty_and_differs() {
        let mut app = App::new();
        app.insert_resource(InstrumentRegistry {
            ids: vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
            editable: true,
        });
        app.insert_resource(ScenarioInstrumentsWritebackState {
            revision: 5,
            flushed_revision: 4,
            last_error: None,
        });
        app.insert_resource(ScenarioMetadata {
            instruments: vec!["OLD.T".to_string()],
            ..Default::default()
        });
        app.add_systems(Update, sync_scenario_metadata_from_registry_system);
        app.update();

        let scen = app.world().resource::<ScenarioMetadata>();
        assert_eq!(
            scen.instruments,
            vec!["1301.TSE".to_string(), "7203.TSE".to_string()]
        );
    }
}
