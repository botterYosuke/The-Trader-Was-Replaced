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
